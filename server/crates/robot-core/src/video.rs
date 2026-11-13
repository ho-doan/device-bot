//! Event-driven Annex-B H.264 subprocess transport shared by native capture drivers.
use crate::{RobotError, VideoFrame, VideoStream};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
fn failure(error: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("Live capture: {error}"))
}
/// The command must emit Annex-B H.264 access units delimited by AUD NALs,
/// including SPS/PPS on keyframes, at 30 fps. Dropping frames stops/reaps it.
pub async fn start_h264_process(
    mut command: Command,
    width: u32,
    height: u32,
) -> Result<VideoStream, RobotError> {
    if width == 0 || height == 0 {
        return Err(RobotError::InvalidInput(
            "Video dimensions must be positive".into(),
        ));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(failure)?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let diagnostics = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let mut chunk = [0; 1024];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > 8192 {
                bytes.drain(..bytes.len() - 8192);
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        let result=async {
            let mut buffer=Vec::new(); let mut chunk=[0;16384]; let mut pts=0;
            loop {
                let n=tokio::select! { _=tx.closed()=>return Ok(()), n=stdout.read(&mut chunk)=>n.map_err(failure)? };
                if n==0 { return Err(failure("capture encoder ended")) }
                buffer.extend_from_slice(&chunk[..n]);
                if buffer.len()>16*1024*1024 { return Err(failure("encoded access unit exceeds 16 MiB")) }
                while let Some(data)=take_access_unit(&mut buffer) {
                    let keyframe=data.windows(4).any(|b| b[..3]==[0,0,1] && b[3]&31==5);
                    if tx.send(Ok(VideoFrame{data,pts_us:pts,keyframe})).await.is_err() { return Ok(()) }
                    pts+=33_333;
                }
            }
        }.await;
        let _ = child.kill().await;
        let log = diagnostics.await.unwrap_or_default();
        if let Err(error) = result {
            let _ = tx.send(Err(failure(format!("{error}; {log}")))).await;
        }
    });
    // Fail setup on missing/old FFmpeg or inaccessible desktop instead of
    // returning a successful SDP for a black viewer.
    let first = tokio::time::timeout(Duration::from_secs(25), rx.recv())
        .await
        .map_err(failure)?
        .ok_or_else(|| failure("capture stopped before its first frame"))??;
    let (ready, frames) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        if ready.send(Ok(first)).await.is_err() {
            return;
        }
        loop {
            let next = tokio::select! { _=ready.closed()=>break, next=rx.recv()=>next };
            let Some(frame) = next else { break };
            if ready.send(frame).await.is_err() {
                break;
            }
        }
    });
    Ok(VideoStream {
        width,
        height,
        frames,
    })
}

fn take_access_unit(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut aud = buffer
        .windows(4)
        .enumerate()
        .filter(|(_, b)| b[..3] == [0, 0, 1] && b[3] & 31 == 9)
        // Annex-B permits both three- and four-byte start codes. Include
        // the leading zero in a four-byte code in the following unit.
        .map(|(i, _)| {
            if i > 0 && buffer[i - 1] == 0 {
                i - 1
            } else {
                i
            }
        });
    aud.next()?;
    let end = aud.next()?;
    Some(buffer.drain(..end).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_start_codes_split_at_access_unit_boundaries() {
        let first = vec![0, 0, 1, 9, 0xf0, 0, 0, 1, 5, 2];
        let second = vec![0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 1, 3];
        let third = vec![0, 0, 1, 9, 0xf0];
        let mut bytes = [first.clone(), second.clone(), third.clone()].concat();
        assert_eq!(take_access_unit(&mut bytes), Some(first));
        assert_eq!(take_access_unit(&mut bytes), Some(second));
        assert_eq!(bytes, third);
    }

    #[test]
    fn partial_access_units_preserve_parameter_sets() {
        let mut bytes = vec![
            0, 0, 0, 1, 9, 0xf0, 0, 0, 0, 1, 7, 1, 0, 0, 0, 1, 8, 1, 0, 0, 0, 1, 5, 2, 0, 0,
        ];
        assert!(take_access_unit(&mut bytes).is_none());
        bytes.extend_from_slice(&[0, 1, 9, 0xf0]);
        let unit = take_access_unit(&mut bytes).unwrap();
        assert!(unit.windows(5).any(|v| v == [0, 0, 0, 1, 7]));
        assert!(unit.windows(5).any(|v| v == [0, 0, 0, 1, 5]));
        assert_eq!(bytes, [0, 0, 0, 1, 9, 0xf0]);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn setup_reports_encoder_failure_instead_of_successful_black_video() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf 'capture permission denied' >&2; exit 2"]);
        let result = start_h264_process(cmd, 320, 240).await;
        assert!(
            matches!(result, Err(error) if error.to_string().contains("capture permission denied"))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn encoder_failure_survives_a_full_frame_queue() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            "for n in 1 2 3 4 5 6; do printf '\\000\\000\\000\\001\\011\\360\\000\\000\\001\\005\\002'; done; printf 'encoder failed' >&2",
        ]);
        let mut stream = start_h264_process(cmd, 320, 240).await.unwrap();
        // Allow capture to fill both bounded queues before consuming them.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let error = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match stream.frames.recv().await {
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => break error,
                    None => panic!("encoder error was lost"),
                }
            }
        })
        .await
        .unwrap();
        assert!(error.to_string().contains("encoder failed"));
    }
}

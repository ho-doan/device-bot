//! Event-driven Annex-B H.264 subprocess transport shared by native capture drivers.
use crate::RobotError;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
/// One complete Annex B H.264 access unit, using the encoder presentation clock.
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub pts_us: u64,
    pub keyframe: bool,
}

pub struct VideoStream {
    pub width: u32,
    pub height: u32,
    pub frames: tokio::sync::mpsc::Receiver<Result<VideoFrame, RobotError>>,
}

/// Browser-native WebRTC media; Rust owns signaling and cancellation.
pub struct NativeVideoStream {
    pub sdp: String,
    pub width: u32,
    pub height: u32,
    pub stop: tokio::sync::watch::Sender<bool>,
    pub done: tokio::task::JoinHandle<()>,
}

fn failure(error: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("Live capture: {error}"))
}
/// The command must emit Annex-B H.264 access units delimited by AUD NALs,
/// including SPS/PPS on keyframes, at 30 fps. Dropping frames stops/reaps it.
pub async fn start_h264_process(
    command: Command,
    width: u32,
    height: u32,
) -> Result<VideoStream, RobotError> {
    if width == 0 || height == 0 {
        return Err(RobotError::InvalidInput(
            "Video dimensions must be positive".into(),
        ));
    }
    start_h264_process_inner(command, Some((width, height)))
        .await?
        .ok_or_else(|| failure("capture source unavailable"))
}

/// Like `start_h264_process`, but the encoder reports width and height as two
/// big-endian u32s on stdout before the first access unit from the same source.
/// A zero pair reports an unavailable source. The encoder should exit when stdin closes.
pub async fn start_h264_process_with_dimensions(
    command: Command,
) -> Result<Option<VideoStream>, RobotError> {
    start_h264_process_inner(command, None).await
}

async fn start_h264_process_inner(
    mut command: Command,
    dimensions: Option<(u32, u32)>,
) -> Result<Option<VideoStream>, RobotError> {
    let input = if dimensions.is_none() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    command
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(failure)?;
    let stdin = child.stdin.take();
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
    let (width, height) = if let Some(dimensions) = dimensions {
        dimensions
    } else {
        let mut header = [0; 8];
        let result = tokio::time::timeout(Duration::from_secs(25), stdout.read_exact(&mut header));
        match result.await {
            Ok(Ok(_)) => {}
            error => {
                let _ = child.kill().await;
                let log = diagnostics.await.unwrap_or_default();
                return Err(failure(format!(
                    "could not read capture dimensions: {error:?}; {log}"
                )));
            }
        }
        let width = u32::from_be_bytes(header[..4].try_into().unwrap());
        let height = u32::from_be_bytes(header[4..].try_into().unwrap());
        if width == 0 && height == 0 {
            let _ = child.kill().await;
            return Ok(None);
        }
        if !(2..=16384).contains(&width) || !(2..=16384).contains(&height) {
            let _ = child.kill().await;
            return Err(failure("invalid capture dimensions"));
        }
        (width, height)
    };
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
        if stdin.is_some() {
            // Let a cooperating encoder release its hardware session. Keep
            // draining stdout so an in-flight encoder callback can finish.
            drop(stdin);
            let drain = async {
                let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
                let _ = child.wait().await;
            };
            let _ = tokio::time::timeout(Duration::from_secs(2), drain).await;
        }
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
    Ok(Some(VideoStream {
        width,
        height,
        frames,
    }))
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
    async fn early_encoder_failure_before_dimensions_preserves_stderr() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf 'capture source disconnected' >&2; exit 2"]);
        let result = start_h264_process_with_dimensions(cmd).await;
        assert!(
            matches!(result, Err(error) if error.to_string().contains("capture source disconnected"))
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

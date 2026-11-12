//! scrcpy adapter: MediaCodec H.264 access units, never screencap polling.
use robot_core::{RobotError, VideoFrame, VideoStream};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::{Child, Command},
    sync::{Mutex, mpsc},
};

/// Control channels follow capture lifetime; no global or stale socket ownership.
#[derive(Debug, Default)]
pub(super) struct Controls(Mutex<HashMap<String, Vec<Weak<Mutex<TcpStream>>>>>);
impl Controls {
    async fn register(&self, id: &str, socket: &Arc<Mutex<TcpStream>>) {
        let mut devices = self.0.lock().await;
        let sockets = devices.entry(id.into()).or_default();
        sockets.retain(|socket| socket.strong_count() > 0);
        sockets.push(Arc::downgrade(socket));
    }
    pub(super) async fn clear_text(&self, id: &str) -> Result<(), RobotError> {
        let socket = self
            .0
            .lock()
            .await
            .get(id)
            .and_then(|sockets| sockets.iter().rev().find_map(Weak::upgrade))
            .ok_or_else(|| {
                RobotError::InvalidInput("Open Android Live before clearing text".into())
            })?;
        let mut socket = socket.lock().await;
        tokio::time::timeout(Duration::from_secs(3), socket.write_all(&clear_text_keys()))
            .await
            .map_err(failure)?
            .map_err(failure)
    }
}

// scrcpy INJECT_KEYCODE: type/u8, action/u8, key/u32, repeat/u32, meta/u32.
// Ctrl+A selects all regardless of text length, cursor, password masking or Unicode.
fn clear_text_keys() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(56);
    for (key, meta) in [(29u32, 0x1000u32), (67, 0)] {
        for action in [0u8, 1] {
            bytes.extend([0, action]);
            bytes.extend(key.to_be_bytes());
            bytes.extend(0u32.to_be_bytes());
            bytes.extend(meta.to_be_bytes());
        }
    }
    bytes
}

fn failure(error: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("scrcpy video: {error}"))
}

async fn adb(adb: &str, id: &str, args: &[&str]) -> Result<String, RobotError> {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new(adb)
            .args(["-s", id])
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| failure("ADB timed out"))?
    .map_err(failure)?;
    if !output.status.success() {
        return Err(failure(String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The guard also covers cancellation during startup and partial failures.
struct Capture {
    adb: String,
    id: String,
    port: String,
    child: Option<Child>,
    control: Option<Arc<Mutex<TcpStream>>>,
}
impl Capture {
    async fn close(&mut self) {
        self.control.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        if !self.port.is_empty() {
            let _ = adb(&self.adb, &self.id, &["forward", "--remove", &self.port]).await;
            self.port.clear();
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.start_kill();
        }
        if self.port.is_empty() {
            return;
        }
        let (adb_path, id, port) = (self.adb.clone(), self.id.clone(), self.port.clone());
        tokio::spawn(async move {
            let _ = adb(&adb_path, &id, &["forward", "--remove", &port]).await;
        });
    }
}

async fn installation() -> Result<(PathBuf, String), RobotError> {
    if let Ok(path) = std::env::var("SCRCPY_SERVER_PATH") {
        let version = std::env::var("SCRCPY_SERVER_VERSION")
            .map_err(|_| failure("set SCRCPY_SERVER_VERSION to match SCRCPY_SERVER_PATH"))?;
        return Ok((path.into(), version));
    }
    let output = Command::new("scrcpy")
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|_| {
            failure("install scrcpy or set SCRCPY_SERVER_PATH and SCRCPY_SERVER_VERSION")
        })?;
    let version = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| failure("could not detect scrcpy version"))?
        .to_owned();
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/share/scrcpy/scrcpy-server"),
        PathBuf::from("/usr/local/share/scrcpy/scrcpy-server"),
        PathBuf::from("/usr/share/scrcpy/scrcpy-server"),
    ];
    if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join("scrcpy-server"))
            .find(|p| p.is_file())
    }) {
        paths.push(path);
    }
    let path = paths
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| failure("set SCRCPY_SERVER_PATH and SCRCPY_SERVER_VERSION"))?;
    Ok((path, version))
}

pub(super) async fn start(
    adb_path: &str,
    id: &str,
    controls: &Controls,
) -> Result<VideoStream, RobotError> {
    let (server, version) = installation().await?;
    // These protocol versions have explicit, tested frame flag layouts.
    let modern = match version.as_str() {
        "3.3.4" => false,
        "5.0.1" => true,
        _ => {
            return Err(failure(format!(
                "unsupported scrcpy {version}; use 3.3.4 or 5.0.1"
            )));
        }
    };
    adb(
        adb_path,
        id,
        &[
            "push",
            server
                .to_str()
                .ok_or_else(|| failure("invalid server path"))?,
            "/data/local/tmp/scrcpy-server.jar",
        ],
    )
    .await?;
    let scid = format!(
        "{:08x}",
        (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(failure)?
            .as_nanos() as u32)
            & 0x7fff_ffff
    );
    let socket = format!("localabstract:scrcpy_{scid}");
    let port = adb(adb_path, id, &["forward", "tcp:0", &socket]).await?;
    let port: u16 = port.parse().map_err(failure)?;
    let mut capture = Capture {
        adb: adb_path.into(),
        id: id.into(),
        port: format!("tcp:{port}"),
        child: None,
        control: None,
    };
    // Keep argv minimal. Redundant default options trigger Samsung Android 10's
    // native ACodec::reconfigEncoder4OtherApps crash on the tested Exynos device.
    let options = [
        format!("scid={scid}"),
        "audio=false".into(),
        "tunnel_forward=true".into(),
        "max_fps=30".into(),
        // Baseline profile matches the WebRTC codec negotiated by the gateway. Periodic
        // IDRs allow late ICE connection and packet loss recovery without decoding here.
        "video_codec_options=profile=1,i-frame-interval=1".into(),
    ];
    let initialized = async {
        capture.child = Some(
            Command::new(adb_path)
                .args([
                    "-s",
                    id,
                    "shell",
                    "CLASSPATH=/data/local/tmp/scrcpy-server.jar",
                    "app_process",
                    "/",
                    "com.genymobile.scrcpy.Server",
                    &version,
                ])
                .args(options)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .map_err(failure)?,
        );
        let mut stream = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(status) = capture
                    .child
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .map_err(failure)?
                {
                    return Err(failure(format!("server exited: {status}")));
                }
                if let Ok(mut socket) = TcpStream::connect(("127.0.0.1", port)).await {
                    let mut dummy = [0];
                    if socket.read_exact(&mut dummy).await.is_ok() && dummy == [0] {
                        break Ok(socket);
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| failure("server startup timed out"))??;
        // scrcpy opens video then control before it sends the device metadata.
        let control = Arc::new(Mutex::new(
            TcpStream::connect(("127.0.0.1", port))
                .await
                .map_err(failure)?,
        ));
        capture.control = Some(control);
        let mut name = [0; 64];
        tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut name))
            .await
            .map_err(failure)?
            .map_err(failure)?;
        let codec = stream.read_u32().await.map_err(failure)?;
        if codec != 0x68323634 {
            return Err(failure("server did not select H.264"));
        }
        if modern {
            let flags = stream.read_u32().await.map_err(failure)?;
            if flags & 0x80000000 == 0 {
                return Err(failure("missing video session header"));
            }
        }
        let width = stream.read_u32().await.map_err(failure)?;
        let height = stream.read_u32().await.map_err(failure)?;
        if width == 0 || height == 0 {
            return Err(failure("invalid video size"));
        }
        Ok::<_, RobotError>((stream, width, height))
    }
    .await;
    let (mut stream, width, height) = match initialized {
        Ok(result) => result,
        Err(error) => {
            capture.close().await;
            return Err(error);
        }
    };
    controls
        .register(
            id,
            capture.control.as_ref().expect("connected control socket"),
        )
        .await;
    let (sender, frames) = mpsc::channel(2);
    tokio::spawn(async move {
        let mut config = Vec::new();
        loop {
            let frame = tokio::select! {
                _ = sender.closed() => break,
                frame = read_frame(&mut stream, modern, &mut config) => frame,
            };
            let failed = frame.is_err();
            if sender.send(frame).await.is_err() || failed {
                break;
            }
        }
        drop(stream);
        capture.close().await;
    });
    Ok(VideoStream {
        width,
        height,
        frames,
    })
}

async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
    modern: bool,
    config: &mut Vec<u8>,
) -> Result<VideoFrame, RobotError> {
    let config_flag = 1u64 << if modern { 62 } else { 63 };
    let key_flag = config_flag >> 1;
    loop {
        let flags = reader.read_u64().await.map_err(failure)?;
        let length = reader.read_u32().await.map_err(failure)? as usize;
        if modern && flags & (1u64 << 63) != 0 {
            config.clear();
            continue;
        }
        if length == 0 || length > 16 * 1024 * 1024 {
            return Err(failure("invalid H.264 packet size"));
        }
        let mut data = vec![0; length];
        reader.read_exact(&mut data).await.map_err(failure)?;
        if flags & config_flag != 0 {
            *config = data;
            continue;
        }
        let keyframe = flags & key_flag != 0;
        if keyframe {
            let mut joined = config.clone();
            joined.extend(data);
            data = joined;
        }
        return Ok(VideoFrame {
            data,
            pts_us: flags & (key_flag - 1),
            keyframe,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn clear_uses_ctrl_a_then_delete_and_rejects_a_closed_capture() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let client = Arc::new(Mutex::new(client));
        let controls = Controls::default();
        controls.register("phone", &client).await;
        controls.clear_text("phone").await.unwrap();
        let mut bytes = [0; 56];
        server.read_exact(&mut bytes).await.unwrap();
        for (index, (key, meta)) in [(29u32, 0x1000u32), (29, 0x1000), (67, 0), (67, 0)]
            .into_iter()
            .enumerate()
        {
            let packet = &bytes[index * 14..(index + 1) * 14];
            assert_eq!(packet[0], 0);
            assert_eq!(packet[1], (index % 2) as u8);
            assert_eq!(&packet[2..6], &key.to_be_bytes());
            assert_eq!(&packet[10..14], &meta.to_be_bytes());
        }
        drop(client);
        assert!(controls.clear_text("phone").await.is_err());
    }

    #[tokio::test]
    async fn parses_fragmented_config_and_keyframes_for_both_protocols() {
        for modern in [false, true] {
            let flag = 1u64 << if modern { 62 } else { 63 };
            let mut bytes = Vec::new();
            for (header, data) in [
                (flag, vec![0, 0, 0, 1, 0x67]),
                ((flag >> 1) | 1234, vec![0, 0, 0, 1, 0x65]),
            ] {
                bytes.extend(header.to_be_bytes());
                bytes.extend((data.len() as u32).to_be_bytes());
                bytes.extend(data);
            }
            let frame = read_frame(&mut bytes.as_slice(), modern, &mut Vec::new())
                .await
                .unwrap();
            assert!(frame.keyframe);
            assert_eq!(frame.pts_us, 1234);
            assert_eq!(frame.data, [0, 0, 0, 1, 0x67, 0, 0, 0, 1, 0x65]);
        }
    }
    #[tokio::test]
    async fn rejects_unbounded_packets_and_truncated_payloads() {
        let mut bytes = vec![0; 8];
        bytes.extend(u32::MAX.to_be_bytes());
        assert!(
            read_frame(&mut bytes.as_slice(), false, &mut Vec::new())
                .await
                .is_err()
        );
        let mut bytes = vec![0; 8];
        bytes.extend(5u32.to_be_bytes());
        bytes.push(0);
        assert!(
            read_frame(&mut bytes.as_slice(), false, &mut Vec::new())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    #[ignore = "requires an attached Android and installed scrcpy 3.3.4 or 5.0.1"]
    async fn physical_device_h264_smoke() {
        let id = std::env::var("DEVICE_BOT_TEST_ANDROID").expect("DEVICE_BOT_TEST_ANDROID");
        let adb_path = std::env::var("ANDROID_ADB_PATH").unwrap_or("adb".into());
        let mut stream = start(&adb_path, &id, &Controls::default()).await.unwrap();
        assert!(stream.width > 0 && stream.height > 0);
        let frame = tokio::time::timeout(Duration::from_secs(10), stream.frames.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(frame.keyframe);
        assert!(frame.data.starts_with(&[0, 0, 0, 1]));
        println!(
            "H.264 {}x{}, first keyframe {} bytes",
            stream.width,
            stream.height,
            frame.data.len()
        );
        drop(stream);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

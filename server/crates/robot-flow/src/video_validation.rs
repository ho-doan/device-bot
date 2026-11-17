//! Validate finalized recording artifacts using the installed FFmpeg decoder.
use crate::{ErrorKind, FlowError};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    time::{Instant, timeout_at},
};

/// Facts measured by fully decoding the first video stream.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoSummary {
    pub decoded_frames: u64,
    pub duration: Duration,
}

fn artifact(message: impl Into<String>) -> FlowError {
    FlowError::new(ErrorKind::Artifact, message)
}

/// Resolve the encoder for a scoped session or a legacy process environment.
pub fn resolve_ffmpeg_path(explicit: Option<&Path>, inherit_process_environment: bool) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_owned();
    }
    if inherit_process_environment {
        if let Some(path) = std::env::var_os("DEVICE_BOT_FFMPEG_PATH").filter(|p| !p.is_empty()) {
            return path.into();
        }
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            for path in [
                directory.join("ffmpeg").join("bin").join(name),
                directory.join(name),
            ] {
                if path.is_file() {
                    return path;
                }
            }
        }
    }
    for path in [
        "/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg",
        "/usr/local/opt/ffmpeg-full/bin/ffmpeg",
        "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
        "/usr/local/opt/ffmpeg/bin/ffmpeg",
    ] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    name.into()
}

/// Fully decode the first video stream before accepting an artifact.
/// `ffmpeg_override` allows a session-scoped encoder path without changing process env.
/// The deadline bounds the child process and cancellation kills the owned child.
pub async fn validate_video(
    path: &Path,
    ffmpeg_override: Option<&Path>,
    deadline: Duration,
) -> Result<VideoSummary, FlowError> {
    validate_video_inner(path, ffmpeg_override, Instant::now() + deadline).await
}

async fn validate_video_inner(
    path: &Path,
    ffmpeg_override: Option<&Path>,
    deadline: Instant,
) -> Result<VideoSummary, FlowError> {
    let metadata = timeout_at(deadline, tokio::fs::metadata(path))
        .await
        .map_err(|_| artifact("recorded video validation timed out"))?
        .map_err(|e| artifact(format!("recorded video metadata: {e}")))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(artifact("recorded video is not a nonempty regular file"));
    }
    // Canonicalization prevents an input beginning with '-' from being interpreted as an option.
    let input = timeout_at(deadline, tokio::fs::canonicalize(path))
        .await
        .map_err(|_| artifact("recorded video validation timed out"))?
        .map_err(|e| artifact(format!("recorded video path: {e}")))?;
    let mut command = Command::new(resolve_ffmpeg_path(ffmpeg_override, true));
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-i",
        ])
        .arg(ffmpeg_input_path(input))
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-f",
            "null",
            "-",
            "-progress",
            "pipe:1",
            "-nostats",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|e| artifact(format!("could not start video validation: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let validation = async {
        let mut buffer = [0u8; 4096];
        let mut line = Vec::with_capacity(128);
        let mut frames = 0;
        let mut micros = 0;
        loop {
            let n = stdout
                .read(&mut buffer)
                .await
                .map_err(|e| artifact(format!("video validation output: {e}")))?;
            if n == 0 {
                break;
            }
            for &byte in &buffer[..n] {
                if byte == b'\n' {
                    if let Ok(text) = std::str::from_utf8(&line) {
                        if let Some(v) = text.strip_prefix("frame=") {
                            frames = v.trim().parse().unwrap_or(0);
                        }
                        if let Some(v) = text.strip_prefix("out_time_us=") {
                            micros = v.trim().parse().unwrap_or(0);
                        }
                    }
                    line.clear();
                } else if line.len() < 1024 {
                    line.push(byte);
                }
            }
        }
        let status = child
            .wait()
            .await
            .map_err(|e| artifact(format!("video validation wait: {e}")))?;
        if !status.success() {
            return Err(artifact(format!("recorded video decode failed ({status})")));
        }
        if frames == 0 || micros == 0 {
            return Err(artifact(
                "recorded video has no decoded frames or positive timeline",
            ));
        }
        Ok(VideoSummary {
            decoded_frames: frames,
            duration: Duration::from_micros(micros),
        })
    };
    match timeout_at(deadline, validation).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(artifact("recorded video validation timed out"))
        }
    }
}

#[cfg(not(windows))]
fn ffmpeg_input_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn ffmpeg_input_path(path: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let extended: Vec<u16> = r"\\?\".encode_utf16().collect();
    let converted = if wide.starts_with(&unc) {
        [vec![b'\\' as u16, b'\\' as u16], wide[unc.len()..].to_vec()].concat()
    } else if wide.starts_with(&extended) && wide.get(5) == Some(&(b':' as u16)) {
        wide[extended.len()..].to_vec()
    } else {
        wide
    };
    std::ffi::OsString::from_wide(&converted).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "video-validation-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    const VIDEO: &[u8] = include_bytes!("../tests/fixtures/tiny.mp4");

    #[cfg(windows)]
    #[test]
    fn ffmpeg_paths_remove_windows_extended_prefixes() {
        assert_eq!(
            ffmpeg_input_path(PathBuf::from(r"\\?\C:\video.mp4")),
            PathBuf::from(r"C:\video.mp4")
        );
        assert_eq!(
            ffmpeg_input_path(PathBuf::from(r"\\?\UNC\host\share\video.mp4")),
            PathBuf::from(r"\\host\share\video.mp4")
        );
    }

    #[tokio::test]
    async fn decodes_real_video_and_rejects_invalid_artifacts() {
        let tmp = Temp::new();
        let file = tmp.0.join("- video.mp4");
        std::fs::write(&file, VIDEO).unwrap();
        let summary = validate_video(&file, None, Duration::from_secs(15))
            .await
            .unwrap();
        assert!(summary.decoded_frames >= 2 && summary.duration > Duration::ZERO);
        for bytes in [&b""[..], &b"not video"[..], &VIDEO[..VIDEO.len() / 2]] {
            std::fs::write(&file, bytes).unwrap();
            assert_eq!(
                validate_video(&file, None, Duration::from_secs(15))
                    .await
                    .unwrap_err()
                    .kind(),
                ErrorKind::Artifact
            );
        }
        assert!(
            validate_video(&tmp.0, None, Duration::from_secs(15))
                .await
                .is_err()
        );
        assert!(
            validate_video(&tmp.0.join("missing"), None, Duration::from_secs(15))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_audio_only_and_zero_duration_media() {
        let tmp = Temp::new();
        let audio = tmp.0.join("audio.wav");
        let zero = tmp.0.join("zero.mp4");
        for (filter, duration, output) in [
            ("anullsrc=r=8000:cl=mono", "0.1", &audio),
            ("color=s=16x16:r=5", "0", &zero),
        ] {
            let status = Command::new(resolve_ffmpeg_path(None, true))
                .args([
                    "-v", "error", "-f", "lavfi", "-i", filter, "-t", duration, "-y",
                ])
                .arg(output)
                .status()
                .await
                .unwrap();
            assert!(status.success());
            assert_eq!(
                validate_video(output, None, Duration::from_secs(15))
                    .await
                    .unwrap_err()
                    .kind(),
                ErrorKind::Artifact
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_failed_decoder_and_kills_it_on_deadline() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = Temp::new();
        let input = tmp.0.join("input");
        std::fs::write(&input, VIDEO).unwrap();
        let executable = tmp.0.join("fake ffmpeg");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf 'frame=3\\nout_time_us=1000000\\n'\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            validate_video(&input, Some(&executable), Duration::from_secs(2))
                .await
                .unwrap_err()
                .to_string()
                .contains("decode failed")
        );
        let pidfile = tmp.0.join("pid");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\necho $$ > '{}'\nexec sleep 30\n",
                pidfile.display()
            ),
        )
        .unwrap();
        let start = std::time::Instant::now();
        assert!(
            validate_video(&input, Some(&executable), Duration::from_millis(100))
                .await
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(pidfile).unwrap();
        let status = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(
            !status.success(),
            "decoder process still exists after timeout"
        );
    }
}

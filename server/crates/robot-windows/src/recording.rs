//! Record the entire Windows desktop, independently of the active UIA target.
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use robot_core::RobotError;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub(super) struct DesktopRecorder {
    active: Option<Recording>,
}

#[derive(Debug)]
struct Recording {
    child: Child,
    path: PathBuf,
    log: PathBuf,
}

impl DesktopRecorder {
    pub async fn start(&mut self, ffmpeg: &str, directory: &Path) -> Result<String, RobotError> {
        if self.active.is_some() {
            return Err(RobotError::InvalidInput(
                "a Windows desktop recording is already in progress".into(),
            ));
        }
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(command_error)?;
        // Avoid canonicalize's Windows \\?\ prefix in paths consumed by
        // FFmpeg and Git Bash clients.
        let directory = std::path::absolute(directory).map_err(command_error)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = directory.join(format!(
            "windows-desktop-{stamp}-{}.mp4",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let log = path.with_extension("ffmpeg.log");
        let stderr = std::fs::File::create(&log).map_err(command_error)?;
        let mut command = Command::new(ffmpeg);
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-n",
                // No title, HWND, PID or region: capture the entire virtual desktop.
                "-f",
                "gdigrab",
                "-framerate",
                "30",
                "-draw_mouse",
                "1",
                "-i",
                "desktop",
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-progress",
                "pipe:1",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW: do not steal UI focus.
        let mut child = command.spawn().map_err(|error| RobotError::CommandFailed(format!(
            "could not start Windows desktop recording; install FFmpeg or set DEVICE_BOT_FFMPEG_PATH: {error}"
        )))?;
        let mut progress =
            BufReader::new(child.stdout.take().expect("piped recorder stdout")).lines();
        // Do not run scenario actions until FFmpeg has encoded its first frame.
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(line) = progress.next_line().await? {
                if line
                    .strip_prefix("frame=")
                    .and_then(|n| n.trim().parse::<u64>().ok())
                    .is_some_and(|n| n > 0)
                {
                    return Ok::<bool, std::io::Error>(true);
                }
            }
            Ok(false)
        })
        .await;
        if !matches!(ready, Ok(Ok(true))) {
            let _ = child.kill().await;
            return Err(RobotError::CommandFailed(format!(
                "Windows desktop recorder did not produce a frame within 10 seconds: {}",
                tokio::fs::read_to_string(&log).await.unwrap_or_default()
            )));
        }
        // Continue draining progress so a long recording cannot block on stdout.
        tokio::spawn(async move { while let Ok(Some(_)) = progress.next_line().await {} });
        self.active = Some(Recording {
            child,
            path: path.clone(),
            log,
        });
        Ok(path.to_string_lossy().into_owned())
    }

    pub async fn stop(&mut self) -> Result<String, RobotError> {
        let Some(mut recording) = self.active.take() else {
            return Err(RobotError::InvalidInput(
                "no Windows desktop recording is in progress".into(),
            ));
        };
        // FFmpeg's stdin command works on Windows and flushes the MP4 trailer.
        // Killing it directly can leave an unplayable file.
        if let Some(mut stdin) = recording.child.stdin.take() {
            let _ = stdin.write_all(b"q\n").await;
        }
        let status =
            match tokio::time::timeout(Duration::from_secs(15), recording.child.wait()).await {
                Ok(result) => result.map_err(command_error)?,
                Err(_) => {
                    let _ = recording.child.kill().await;
                    return Err(RobotError::CommandFailed(
                        "Windows desktop recorder did not finalize within 15 seconds".into(),
                    ));
                }
            };
        let size = tokio::fs::metadata(&recording.path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if !status.success() || size == 0 {
            return Err(RobotError::CommandFailed(format!(
                "Windows desktop recording failed ({status}): {}",
                tokio::fs::read_to_string(&recording.log)
                    .await
                    .unwrap_or_default()
            )));
        }
        Ok(recording.path.to_string_lossy().into_owned())
    }
}

fn command_error(error: std::io::Error) -> RobotError {
    RobotError::CommandFailed(format!("Windows desktop recording: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    async fn fixture(script: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "windows-recorder-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let executable = dir.join("ffmpeg");
        tokio::fs::write(&executable, format!("#!/bin/sh\n{script}\n"))
            .await
            .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        (dir, executable)
    }

    #[tokio::test]
    async fn desktop_recording_finalizes_on_quit_and_can_restart() {
        let (dir, executable) = fixture("for arg do target=$arg; done\nprintf '%s\\n' \"$@\" > \"$target.args\"\necho frame=1\nread -r quit\n[ \"$quit\" = q ] || exit 1\nprintf video > \"$target\"").await;
        let mut recorder = DesktopRecorder::default();
        let path = recorder
            .start(executable.to_str().unwrap(), &dir)
            .await
            .unwrap();
        assert!(
            recorder
                .start(executable.to_str().unwrap(), &dir)
                .await
                .is_err()
        );
        assert_eq!(recorder.stop().await.unwrap(), path);
        let args = tokio::fs::read_to_string(format!("{path}.args"))
            .await
            .unwrap();
        assert!(args.contains("gdigrab\n"));
        assert!(args.contains("-i\ndesktop\n"));
        assert!(!args.contains("title="));
        assert!(recorder.stop().await.is_err());
        let second = recorder
            .start(executable.to_str().unwrap(), &dir)
            .await
            .unwrap();
        assert_ne!(path, second);
        recorder.stop().await.unwrap();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn startup_failure_is_reported_without_retaining_session() {
        let (dir, executable) = fixture("echo no-desktop >&2\nexit 1").await;
        let mut recorder = DesktopRecorder::default();
        let error = recorder
            .start(executable.to_str().unwrap(), &dir)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no-desktop"));
        assert!(recorder.active.is_none());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn missing_artifact_does_not_report_success() {
        let (dir, executable) = fixture("echo frame=1\nread -r quit\nexit 0").await;
        let mut recorder = DesktopRecorder::default();
        recorder
            .start(executable.to_str().unwrap(), &dir)
            .await
            .unwrap();
        assert!(recorder.stop().await.is_err());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}

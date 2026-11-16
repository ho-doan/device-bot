//! Bounded lifecycle for the FFmpeg process owned by one macOS session.
use robot_core::RobotError;
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Child,
};

#[derive(Debug)]
pub(super) struct ActiveRecording {
    pub path: PathBuf,
    pub child: Child,
}

fn failure(message: impl Into<String>) -> RobotError {
    RobotError::CommandFailed(message.into())
}

async fn reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}

pub(super) async fn ready(child: &mut Child, timeout: Duration) -> Result<(), RobotError> {
    let Some(mut progress) = child.stdout.take() else {
        reap(child).await;
        return Err(failure("macOS recorder has no progress stream"));
    };
    // FFmpeg's progress protocol emits frame=<count> only after encoding.
    // Keep at most one short line, even if a broken process emits endless data.
    let result = tokio::time::timeout(timeout, async {
        let mut line = Vec::with_capacity(128);
        let mut buffer = [0; 1024];
        loop {
            let n = progress.read(&mut buffer).await?;
            if n == 0 {
                return Ok::<bool, std::io::Error>(false);
            }
            for &byte in &buffer[..n] {
                if byte == b'\n' {
                    if line
                        .strip_prefix(b"frame=")
                        .and_then(|value| std::str::from_utf8(value).ok())
                        .and_then(|value| value.trim().parse::<u64>().ok())
                        .is_some_and(|value| value > 0)
                    {
                        return Ok(true);
                    }
                    line.clear();
                } else if line.len() < 128 {
                    line.push(byte);
                }
            }
        }
    })
    .await;
    if !matches!(result, Ok(Ok(true))) {
        reap(child).await;
        return Err(failure(
            "macOS desktop recorder did not produce an encoded frame before its startup deadline",
        ));
    }
    if let Some(status) = child
        .try_wait()
        .map_err(|e| failure(format!("could not inspect macOS recorder: {e}")))?
    {
        return Err(failure(format!(
            "macOS desktop recorder exited during startup ({status})"
        )));
    }
    // Drain without accumulating logs for the lifetime of the recording.
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut progress, &mut tokio::io::sink()).await;
    });
    Ok(())
}

pub(super) async fn finish(
    mut recording: ActiveRecording,
    timeout: Duration,
) -> Result<String, RobotError> {
    if let Some(mut stdin) = recording.child.stdin.take() {
        let _ = tokio::time::timeout(Duration::from_secs(1), stdin.write_all(b"q\n")).await;
    }
    let status = match tokio::time::timeout(timeout, recording.child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            reap(&mut recording.child).await;
            return Err(failure(format!(
                "could not finalize macOS screen recording: {error}"
            )));
        }
        Err(_) => {
            reap(&mut recording.child).await;
            return Err(failure(
                "macOS desktop recorder exceeded its finalization deadline",
            ));
        }
    };
    let size = tokio::fs::metadata(&recording.path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if !status.success() || size == 0 {
        return Err(failure(format!(
            "macOS desktop recording failed ({status}); artifact is missing or empty, or FFmpeg exited unsuccessfully"
        )));
    }
    Ok(recording.path.to_string_lossy().into_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        process::Stdio,
        sync::atomic::{AtomicU64, Ordering},
    };
    use tokio::process::Command;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "macos-recording-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn child(script: &str, path: &PathBuf) -> Child {
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .arg("fixture")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    #[tokio::test]
    async fn waits_for_encoded_frame_and_gracefully_finalizes() {
        let path = path();
        let mut child = child(
            "sleep 0.15; echo frame=1; read -r quit; [ \"$quit\" = q ] || exit 1; printf video > \"$1\"",
            &path,
        );
        let start = tokio::time::Instant::now();
        ready(&mut child, Duration::from_secs(2)).await.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(100));
        assert_eq!(
            finish(
                ActiveRecording {
                    child,
                    path: path.clone()
                },
                Duration::from_secs(2)
            )
            .await
            .unwrap(),
            path.to_string_lossy()
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"video");
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn closing_fork_preserves_parent_and_parent_close_finalizes() {
        use robot_core::Robot;
        let path = path();
        let robot = crate::MacosRobot::new(std::env::temp_dir());
        let mut process = child(
            "echo frame=1; read -r quit; [ \"$quit\" = q ] || exit 1; printf video > \"$1\"",
            &path,
        );
        ready(&mut process, Duration::from_secs(2)).await.unwrap();
        *robot.recording.lock().await = Some(ActiveRecording {
            child: process,
            path: path.clone(),
        });
        let fork = robot.fork_session().unwrap();
        fork.close_session().await.unwrap();
        assert!(!path.exists());
        assert!(robot.recording.lock().await.is_some());
        robot.close_session().await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"video");
        robot.close_session().await.unwrap();
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn startup_timeout_reaps_child_and_zero_frame_is_not_ready() {
        let path = path();
        let mut child = child("echo frame=0; exec sleep 60", &path);
        let pid = child.id().unwrap();
        assert!(ready(&mut child, Duration::from_millis(100)).await.is_err());
        assert!(child.try_wait().unwrap().is_some());
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }

    #[tokio::test]
    async fn finalization_timeout_reaps_and_nonzero_exit_or_empty_artifact_fails() {
        let path = path();
        let mut stalled = child("echo frame=1; exec sleep 60", &path);
        ready(&mut stalled, Duration::from_secs(2)).await.unwrap();
        let pid = stalled.id().unwrap();
        assert!(
            finish(
                ActiveRecording {
                    child: stalled,
                    path: path.clone()
                },
                Duration::from_millis(100)
            )
            .await
            .is_err()
        );
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        for script in [
            "echo frame=1; read -r quit; printf video > \"$1\"; exit 1",
            "echo frame=1; read -r quit; : > \"$1\"",
        ] {
            let mut child = child(script, &path);
            ready(&mut child, Duration::from_secs(2)).await.unwrap();
            assert!(
                finish(
                    ActiveRecording {
                        child,
                        path: path.clone()
                    },
                    Duration::from_secs(2)
                )
                .await
                .is_err()
            );
        }
        let _ = tokio::fs::remove_file(path).await;
    }
}

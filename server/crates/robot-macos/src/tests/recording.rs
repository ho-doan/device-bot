use super::*;

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires an existing Screen Recording grant and interactive desktop"]
async fn live_desktop_recording_smoke() {
    assert!(
        unsafe { CGPreflightScreenCaptureAccess() },
        "live desktop recording smoke requires an existing Screen Recording grant"
    );
    let dir = std::env::temp_dir().join(format!("macos-live-recording-{}", std::process::id()));
    let robot = MacosRobot::new(&dir);
    let path = robot.start_screen_recording(DEVICE_ID).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(robot.stop_screen_recording(DEVICE_ID).await.unwrap(), path);
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-i", &path, "-f", "null", "-"])
        .status()
        .await
        .unwrap();
    assert!(status.success(), "recording could not be decoded");
    tokio::fs::remove_dir_all(dir).await.unwrap();
    eprintln!("LIVE_SMOKE_PASSED: encoded desktop recording decoded successfully");
}

#[cfg(unix)]
#[tokio::test]
async fn fork_cannot_stop_parent_recording() {
    let robot = MacosRobot::new(std::env::temp_dir());
    let path = std::env::temp_dir().join(format!("macos-owner-test-{}", std::process::id()));
    std::fs::write(&path, b"recording").unwrap();
    let child = Command::new("sleep")
        .arg("60")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    *robot.recording.lock().await = Some(ActiveRecording {
        path: path.clone(),
        child,
    });
    let fork = robot.fork_session().unwrap();
    assert!(fork.stop_screen_recording(DEVICE_ID).await.is_err());
    assert!(robot.recording.lock().await.is_some());
    let _ = std::fs::remove_file(path);
}

use super::*;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
#[ignore = "requires an interactive macOS session"]
async fn real_focus_restores_the_foreground_pid_without_applescript_coercion() {
    let robot = MacosRobot::new(std::env::temp_dir());
    let previous = robot.focus_command("frontmost", "").await.unwrap();
    let restored = robot.focus_command("restore", &previous.to_string()).await;
    assert_eq!(restored.unwrap(), previous);
}

#[tokio::test]
#[ignore = "requires an interactive macOS session and Screen Recording permission"]
async fn real_screenshot_returns_png_and_restores_the_foreground_app() {
    let directory =
        std::env::temp_dir().join(format!("real-macos-screenshot-{}", std::process::id()));
    let robot = MacosRobot::new(&directory);
    let previous = robot.focus_command("frontmost", "").await.unwrap();
    let app = robot.run("osascript", &[
        "-l".into(), "JavaScript".into(),
        "-e".into(),
        "ObjC.import('AppKit'); ObjC.unwrap($.NSWorkspace.sharedWorkspace.frontmostApplication.bundleIdentifier);".into(),
    ]).await.unwrap();
    *robot.active_application.lock().await =
        Some(String::from_utf8(app).unwrap().trim().to_owned());
    let image = robot.take_screenshot(DEVICE_ID).await;
    let foreground = robot.focus_command("frontmost", "").await.unwrap();
    let _ = std::fs::remove_dir_all(directory);
    assert_eq!(foreground, previous);
    assert!(image.unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
#[ignore = "requires DEVICE_BOT_MACOS_TEST_QC_APP, an interactive session and Screen Recording"]
async fn real_screenshot_switches_from_qc_to_finder_and_restores_qc() {
    let qc =
        std::env::var("DEVICE_BOT_MACOS_TEST_QC_APP").expect("set the path to a built QC .app");
    let directory = std::env::temp_dir().join(format!("qc-focus-roundtrip-{}", std::process::id()));
    let robot = MacosRobot::new(&directory);
    robot.launch_app(DEVICE_ID, &qc).await.unwrap();
    let qc_pid = robot.active_process_id.lock().await.unwrap();
    let image = robot
        .take_screenshot_with_options(
            DEVICE_ID,
            &robot_core::ScreenshotOptions {
                app_id: Some("com.apple.finder".into()),
                display_id: None,
            },
        )
        .await;
    let foreground = robot.focus_command("frontmost", "").await.unwrap();
    let _ = std::fs::remove_dir_all(directory);
    assert_eq!(foreground, qc_pid, "screenshot must return focus to QC");
    assert!(image.unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
async fn screenshot_focuses_target_then_restores_qc_even_on_capture_error() {
    let dir = std::env::temp_dir().join(format!("desktop-focus-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let focus = dir.join("focus");
    std::fs::write(&focus, "QC").unwrap();
    let osa = dir.join("osascript");
    let capture = dir.join("screencapture");
    std::fs::write(
        &osa,
        format!(
            r#"#!/usr/bin/env python3
import sys,pathlib
p=pathlib.Path({focus:?})
a=sys.argv[sys.argv.index('--')+1:]
if a[0]=='frontmost': print('100')
elif a[0]=='activate': p.write_text('TARGET'); print('200')
elif a[0]=='restore': p.write_text('QC'); print('100')
"#,
            focus = focus.to_string_lossy()
        ),
    )
    .unwrap();
    std::fs::write(
        &capture,
        format!(
            r#"#!/usr/bin/env python3
import sys,pathlib
state=pathlib.Path({focus:?}).read_text()
pathlib.Path(sys.argv[-1]).write_bytes(state.encode())
if pathlib.Path({fail:?}).exists(): sys.exit(8)
"#,
            focus = focus.to_string_lossy(),
            fail = dir.join("fail").to_string_lossy()
        ),
    )
    .unwrap();
    for path in [&osa, &capture] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut robot = MacosRobot::new(&dir);
    robot.osascript_path = osa.to_string_lossy().into_owned();
    robot.screencapture_path = capture.to_string_lossy().into_owned();
    *robot.active_application.lock().await = Some("Target".into());
    assert_eq!(robot.take_screenshot(DEVICE_ID).await.unwrap(), b"TARGET");
    assert_eq!(std::fs::read_to_string(&focus).unwrap(), "QC");
    std::fs::write(dir.join("fail"), "").unwrap();
    assert!(robot.take_screenshot(DEVICE_ID).await.is_err());
    assert_eq!(std::fs::read_to_string(&focus).unwrap(), "QC");
    assert!(!std::fs::read_dir(&dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "png")
    }));
    std::fs::remove_dir_all(&dir).unwrap();
}

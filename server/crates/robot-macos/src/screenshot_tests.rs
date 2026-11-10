use super::*;
use std::os::unix::fs::PermissionsExt;

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

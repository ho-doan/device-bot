use super::*;
use robot_core::DeviceState;

#[cfg(unix)]
#[tokio::test]
async fn start_waits_for_simctl_bootstatus_and_propagates_boot_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "device-bot-simulator-start-{}-{}",
        std::process::id(),
        ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("xcrun");
    std::fs::write(&script, "#!/bin/sh\n[ \"$1\" = simctl ] && [ \"$2\" = bootstatus ] && [ \"$4\" = -b ] || exit 7\n[ \"$3\" = failed-id ] && { echo 'boot failed' >&2; exit 9; }\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let robot = IosSimulatorRobot::new(script.to_string_lossy(), &dir);
    robot.boot_device("simulator-id").await.unwrap();
    let error = robot.boot_device("failed-id").await.unwrap_err();
    assert!(error.to_string().contains("boot failed"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn terminate_stops_only_the_named_app_and_rejects_invalid_ids() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "device-bot-simulator-terminate-{}-{}",
        std::process::id(),
        ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("xcrun");
    std::fs::write(&script, "#!/bin/sh\n[ \"$1\" = simctl ] && [ \"$2\" = terminate ] && [ \"$3\" = simulator-id ] && [ \"$4\" = com.example.App ] || exit 7\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let robot = IosSimulatorRobot::new(script.to_string_lossy(), &dir);
    robot
        .active_apps
        .lock()
        .await
        .insert("simulator-id".into(), "com.example.App".into());
    robot
        .terminate_app("simulator-id", "com.example.App")
        .await
        .unwrap();
    assert!(robot.active_apps.lock().await.get("simulator-id").is_none());
    assert!(matches!(
        robot.terminate_app("simulator-id", "--bad").await,
        Err(RobotError::InvalidInput(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn parses_booted_and_shutdown_simulators() {
    let devices = parse_simctl_devices(r#"{"devices":{"runtime":[{"udid":"booted-id","name":"iPhone 16","state":"Booted","isAvailable":true},{"udid":"off-id","name":"iPhone SE","state":"Shutdown","isAvailable":true}]}}"#).unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].state, DeviceState::Available);
    assert_eq!(devices[1].state, DeviceState::Offline);
}

#[test]
fn recording_paths_are_unique_and_stay_in_the_artifact_directory() {
    let robot = IosSimulatorRobot::new("xcrun", "artifacts");
    let first = robot.next_recording_path("test-device");
    let second = robot.next_recording_path("test-device");
    assert!(first.starts_with("artifacts"));
    assert_eq!(
        first.extension().and_then(|value| value.to_str()),
        Some("mp4")
    );
    assert_ne!(first, second);
}

#[test]
fn resolves_relative_xctest_artifact_paths_to_absolute_paths() {
    let path = IosSimulatorRobot::absolute_path(Path::new("artifacts")).unwrap();

    assert!(path.is_absolute());
    assert!(path.ends_with("artifacts"));
}

#[test]
fn decodes_xctest_accessibility_elements() {
    let element: Value = serde_json::from_str(
        r#"{"nodeId":"ios-1-0","automationId":"save-button","text":"Save","label":"Save changes","value":null,"role":"button","bounds":{"x":20,"y":100,"width":80,"height":44},"clickable":true,"enabled":true}"#,
    )
    .unwrap();
    let elements = decode_agent_elements(&json!({ "elements": [element] })).unwrap();
    assert_eq!(elements.len(), 1);
    let element = &elements[0];
    assert_eq!(element.class_name, "button");
    assert_eq!(element.resource_id.as_deref(), Some("save-button"));
    assert_eq!(element.content_description.as_deref(), Some("Save changes"));
    assert_eq!(
        (
            element.bounds.left,
            element.bounds.top,
            element.bounds.right,
            element.bounds.bottom,
        ),
        (20, 100, 100, 144)
    );
    assert!(element.clickable);
    assert!(element.enabled);
}

#[test]
fn rejects_unsafe_simulator_ids_and_invalid_screen_points() {
    assert!(IosSimulatorRobot::valid_device_id("--help").is_err());
    assert!(
        IosSimulatorRobot::valid_point(
            Point {
                x: f64::NAN,
                y: 0.0
            },
            "tap"
        )
        .is_err()
    );
    assert!(IosSimulatorRobot::valid_point(Point { x: -1.0, y: 0.0 }, "tap").is_err());
    assert!(IosSimulatorRobot::valid_point(Point { x: 12.6, y: 24.2 }, "tap").is_ok());
}

#[test]
fn xctest_runner_cache_requires_both_app_products() {
    let derived_data = std::env::temp_dir().join(format!(
        "mobile-mcp-ios-xctest-cache-test-{}",
        ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let products = derived_data.join("Build/Products/Debug-iphonesimulator");
    std::fs::create_dir_all(&products).unwrap();
    std::fs::write(
        derived_data.join("Build/Products/stale.xctestrun"),
        "stale marker",
    )
    .unwrap();

    assert!(!IosSimulatorRobot::xctest_runner_ready(&derived_data));
    std::fs::create_dir(products.join("MobileMcpSimulatorRunnerHost.app")).unwrap();
    assert!(!IosSimulatorRobot::xctest_runner_ready(&derived_data));
    std::fs::create_dir(products.join("MobileMcpSimulatorRunnerUITests-Runner.app")).unwrap();
    assert!(IosSimulatorRobot::xctest_runner_ready(&derived_data));

    std::fs::remove_dir_all(derived_data).unwrap();
}

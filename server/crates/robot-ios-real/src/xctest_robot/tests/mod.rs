use super::*;
use robot_core::{Bounds, Platform};

#[test]
fn trust_warning_uses_the_latest_launch_and_never_overrides_a_live_helper() {
    let trust = "Command line invocation:\nxcodebuild test-without-building\nVerify that the Developer App certificate for your account is trusted on your device";
    let mut status = UiRunnerStatus {
        state: "disconnected".into(),
        ..Default::default()
    };
    apply_runner_warning(&mut status, trust);
    assert_eq!(status.state, "trust_required");
    assert!(!status.ready);
    assert!(status.detail.contains("VPN & Device Management"));
    assert!(status.detail.contains("Retry after Trust"));
    let mut ready = UiRunnerStatus {
        ready: true,
        state: "ready".into(),
        ..Default::default()
    };
    apply_runner_warning(&mut ready, trust);
    assert!(ready.ready);
    assert_eq!(ready.state, "ready");
    for log in [
        "Unable to install: device locked".to_owned(),
        format!("{trust}\nCommand line invocation:\nxcodebuild\nerror: device offline"),
    ] {
        let mut status = UiRunnerStatus {
            state: "disconnected".into(),
            ..Default::default()
        };
        apply_runner_warning(&mut status, &log);
        assert_eq!(status.state, "disconnected");
    }
}

#[test]
fn bridge_warning_uses_transport_evidence_and_does_not_override_a_heartbeat() {
    let launch = "Command line invocation:\nxcodebuild test-without-building\n";
    let timeout = "Device Bot bridge request failed (GET http://192.168.80.36:18797/next?deviceId=private-device): The request timed out.";
    let unreachable = "Device Bot bridge unreachable (http://[fd00::1]:18797/next)";
    for (line, address) in [
        (timeout, "192.168.80.36:18797"),
        (unreachable, "[fd00::1]:18797"),
    ] {
        let mut status = UiRunnerStatus {
            state: "disconnected".into(),
            ..Default::default()
        };
        apply_runner_warning(&mut status, &format!("{launch}{line}"));
        assert_eq!(status.state, "bridge_unreachable");
        assert!(status.detail.contains(address));
        assert!(status.detail.contains("same local network"));
        assert!(!status.detail.contains("private-device"));
        assert!(!status.ready);
    }
    for log in [
        format!("{launch}{timeout}\n{launch}error: device offline"),
        format!("{launch}{unreachable}\nDevice Bot bridge reachable"),
        format!(
            "{launch}Device Bot bridge request failed (GET http://192.168.80.36:18797/next): unauthorized"
        ),
        format!(
            "{launch}Device Bot bridge request failed (POST http://192.168.80.36:18797/result): The request timed out."
        ),
    ] {
        let mut status = UiRunnerStatus {
            state: "disconnected".into(),
            ..Default::default()
        };
        apply_runner_warning(&mut status, &log);
        assert_eq!(status.state, "disconnected", "{log}");
    }
    let mut connected = UiRunnerStatus {
        state: "needs_target".into(),
        ..Default::default()
    };
    apply_runner_warning(&mut connected, &format!("{launch}{timeout}"));
    assert_eq!(connected.state, "needs_target");
}

#[cfg(unix)]
#[tokio::test]
async fn public_health_exposes_runner_launch_warnings_for_qc() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("ios-trust-{}", Uuid::new_v4()));
    let project = root.join("Phone.xcodeproj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("project.pbxproj"),
        "DEVELOPMENT_TEAM = TEAM123456;",
    )
    .unwrap();
    let xcrun = root.join("xcrun");
    let device = "00008130-000959E43EDA001C";
    let devices = json!({"result":{"devices":[{"identifier":"CORE-ID",
        "deviceProperties":{"name":"Phone"},"connectionProperties":{
            "tunnelState":"connected","pairingState":"paired",
            "potentialHostnames":[format!("{device}.coredevice.local")]}}]}});
    std::fs::write(&xcrun, format!("#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\nprintf '%s' '{}' > \"$last\"\n", devices)).unwrap();
    std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
    let logdir = root.join("build/Logs/Agent");
    std::fs::create_dir_all(&logdir).unwrap();
    std::fs::write(
        logdir.join(format!("{device}-xcodebuild.log")),
        "Command line invocation:\nxcodebuild\nprofile has not been explicitly trusted by the user",
    )
    .unwrap();
    let robot = IosXCTestRobot::with_artifact_dir(
        xcrun.to_string_lossy(),
        XCTestAgentBridge::new(None),
        root.join("artifacts"),
    )
    .with_runner_project(project);
    let status = robot.check_device_health(device).await.unwrap();
    assert_eq!(status.state, "trust_required");
    assert!(!status.ready);
    assert_eq!(status.diagnostics.unwrap()["connected"], true);
    std::fs::write(
        logdir.join(format!("{device}-xcodebuild.log")),
        "Command line invocation:\nxcodebuild\nDevice Bot bridge unreachable (http://192.168.80.36:18797/next)",
    ).unwrap();
    let status = robot.check_device_health(device).await.unwrap();
    assert_eq!(status.state, BRIDGE_UNREACHABLE);
    assert!(!status.ready);
    assert!(status.detail.contains("192.168.80.36:18797"));
    assert_eq!(status.diagnostics.unwrap()["connected"], true);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn locked_phone_is_reported_by_health_and_blocks_ui_tools_before_helper_start() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("ios-lock-{}", Uuid::new_v4()));
    let project = root.join("Phone.xcodeproj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("project.pbxproj"),
        "DEVELOPMENT_TEAM = TEAM123456;",
    )
    .unwrap();
    let device = "00008130-000959E43EDA001C";
    let devices = json!({"result":{"devices":[{"identifier":"CORE-ID",
        "deviceProperties":{"name":"Phone"},"connectionProperties":{
            "tunnelState":"connected","pairingState":"paired",
            "potentialHostnames":[format!("{device}.coredevice.local")]}}]}});
    let locked = root.join("locked.json");
    std::fs::write(&locked, r#"{"info":{"outcome":"success"},"result":{"deviceIdentifier":"CORE-ID","passcodeRequired":true,"unlockedSinceBoot":true}}"#).unwrap();
    let xcrun = root.join("xcrun");
    std::fs::write(&xcrun, format!(
        "#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\ncase \"$*\" in\n  'devicectl device info lockState '*) cat '{}' > \"$last\" ;;\n  'devicectl list devices '*) printf '%s' '{}' > \"$last\" ;;\n  *) exit 1 ;;\nesac\n", locked.display(), devices
    )).unwrap();
    std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
    let robot = IosXCTestRobot::with_artifact_dir(
        xcrun.to_string_lossy(),
        XCTestAgentBridge::new(None),
        root.join("artifacts"),
    )
    .with_runner_project(project);
    let status = robot.check_device_health(device).await.unwrap();
    assert_eq!(status.state, "device_locked", "{}", status.detail);
    assert!(!status.ready);
    assert!(status.detail.contains("Unlock"));
    assert_eq!(status.diagnostics.unwrap()["screenLocked"], true);
    let error = robot.take_screenshot(device).await.unwrap_err().to_string();
    assert!(error.starts_with("device_locked:"), "{error}");
    assert!(robot.persistent_agents.lock().await.is_empty());
    std::fs::write(&locked, r#"{"info":{"outcome":"success"},"result":{"deviceIdentifier":"CORE-ID","passcodeRequired":false,"unlockedSinceBoot":true}}"#).unwrap();
    let status = robot.check_device_health(device).await.unwrap();
    assert_ne!(status.state, "device_locked");
    assert_eq!(status.diagnostics.unwrap()["screenLocked"], false);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn parses_coredevice_json_to_xcode_udids() {
    let devices = parse_devicectl_devices(r#"{"result":{"devices":[{"identifier":"F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6","deviceProperties":{"name":"ho-doan"},"connectionProperties":{"potentialHostnames":["00008130-000959E43EDA001C.coredevice.local","F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6.coredevice.local"]}}]}}"#).unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, "00008130-000959E43EDA001C");
    assert_eq!(devices[0].platform, Platform::Ios);
}

fn element(
    class_name: &str,
    resource_id: Option<&str>,
    content_description: Option<&str>,
    clickable: bool,
    enabled: bool,
) -> UiElement {
    UiElement {
        class_name: class_name.into(),
        resource_id: resource_id.map(str::to_owned),
        text: None,
        content_description: content_description.map(str::to_owned),
        bounds: Bounds {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        clickable,
        enabled,
        focused: None,
        checked: None,
        selected: None,
        value: None,
        element_type: None,
        web_context: None,
    }
}

#[test]
fn tap_element_selection_matches_simulator_semantics() {
    let elements = vec![
        element("textField", None, Some("Private"), true, true),
        element(
            "button",
            Some("PrivateTabGroup"),
            Some("Private, Tab Group"),
            true,
            true,
        ),
        element(
            "button",
            Some("DisabledPrivate"),
            Some("Private"),
            true,
            false,
        ),
    ];

    let selected = select_tappable_element(&elements, "Private, Tab Group", true).unwrap();
    assert_eq!(selected.resource_id.as_deref(), Some("PrivateTabGroup"));
}

#[test]
fn tap_element_selection_rejects_non_hittable_elements() {
    let elements = vec![element(
        "button",
        Some("DoneButton"),
        Some("Done"),
        false,
        true,
    )];

    assert!(select_tappable_element(&elements, "Done", true).is_none());
}

#[test]
fn coordinate_selection_keeps_an_enabled_control_that_is_not_hittable() {
    let elements = vec![element(
        "button",
        Some("action-button"),
        Some("Action"),
        false,
        true,
    )];

    let selected = select_tappable_element(&elements, "Action", false).unwrap();
    assert_eq!(selected.resource_id.as_deref(), Some("action-button"));
    assert!(select_tappable_element(&elements, "Action", true).is_none());
}

#[test]
fn core_launcher_preserves_a_raw_ipv6_advertised_host() {
    let launcher = XCTestAgentLauncher::new(
        "xcodebuild",
        "runner.xcodeproj",
        "derived-data",
        "fd54:ebfd:3d4f::2",
        18787,
        Some("token".into()),
    )
    .unwrap();

    assert_eq!(launcher.hosts, "fd54:ebfd:3d4f::2");
    assert_eq!(launcher.port, 18787);
    assert_eq!(launcher.token.as_deref(), Some("token"));
    assert_eq!(
        launcher.test_environment(),
        vec![
            (
                "TEST_RUNNER_DEVICE_BOT_HOSTS".into(),
                "fd54:ebfd:3d4f::2".into()
            ),
            ("TEST_RUNNER_DEVICE_BOT_PORT".into(), "18787".into()),
            ("TEST_RUNNER_DEVICE_BOT_TOKEN".into(), "token".into()),
        ]
    );
}

#[test]
fn core_launcher_rejects_a_loopback_or_link_local_callback_host() {
    for host in ["127.0.0.1", "0.0.0.0", "169.254.1.10", "::1", "fe80::1"] {
        let error = XCTestAgentLauncher::new(
            "xcodebuild",
            "runner.xcodeproj",
            "derived-data",
            host,
            18787,
            Some("token".into()),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("DEVICE_BOT_IOS_AGENT_HOSTS"),
            "{host}"
        );
    }
}

#[test]
fn core_launcher_rejects_a_host_that_is_not_a_host_value() {
    let error = XCTestAgentLauncher::new(
        "xcodebuild",
        "runner.xcodeproj",
        "derived-data",
        "http://192.168.50.56/next",
        18787,
        None,
    )
    .unwrap_err();

    assert!(error.to_string().contains("DEVICE_BOT_IOS_AGENT_HOSTS"));
}
#[test]
fn unavailable_or_unpaired_coredevices_are_never_available() {
    for (tunnel, pairing, expected) in [
        ("unavailable", "paired", DeviceState::Offline),
        ("disconnected", "paired", DeviceState::Offline),
        ("connected", "unpaired", DeviceState::Unauthorized),
        ("connected", "paired", DeviceState::Available),
        ("connecting", "paired", DeviceState::Unknown),
    ] {
        let devices = parse_devicectl_devices(&json!({"result":{"devices":[{
            "identifier":"CORE-ID", "deviceProperties":{"name":"Phone"},
            "connectionProperties":{"potentialHostnames":["00008130-000959E43EDA001C.coredevice.local"],
                "tunnelState":tunnel, "pairingState":pairing}
        }]}}).to_string()).unwrap();
        assert_eq!(devices[0].state, expected, "{tunnel}/{pairing}");
    }
}
#[cfg(unix)]
#[tokio::test]
async fn public_prepare_and_reboot_fail_before_build_when_device_is_unavailable() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("ios-preflight-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let xcrun = root.join("xcrun");
    let fixture = json!({"result":{"devices":[{"identifier":"CORE-ID","deviceProperties":{"name":"Phone"},"connectionProperties":{"tunnelState":"unavailable","pairingState":"paired","potentialHostnames":["00008130-000959E43EDA001C.coredevice.local"]}}]}}).to_string();
    std::fs::write(&xcrun, format!("#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\nprintf '%s' '{}' > \"$last\"\n", fixture)).unwrap();
    std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("xctest-runner/MobileMcpRealRunner.xcodeproj");
    let launcher = XCTestAgentLauncher::new(
        "must-not-run-xcodebuild",
        project,
        root.join("build"),
        "192.168.1.2",
        18787,
        Some("test-token".into()),
    )
    .unwrap();
    let robot = IosXCTestRobot::with_artifact_dir(
        xcrun.to_string_lossy(),
        XCTestAgentBridge::new(None),
        root.join("artifacts"),
    )
    .with_agent_launcher(launcher);
    let status = robot
        .check_device_health("00008130-000959E43EDA001C")
        .await
        .unwrap();
    assert!(!status.ready);
    assert_eq!(status.state, "offline");
    assert_eq!(status.diagnostics.unwrap()["connected"], false);
    for reinstall in [false, true] {
        let result = if reinstall {
            robot.reboot_helper("00008130-000959E43EDA001C", None).await
        } else {
            robot
                .prepare_helper("00008130-000959E43EDA001C", None)
                .await
        };
        let error = result.unwrap_err().to_string();
        assert!(error.contains("CoreDevice"), "{error}");
        assert!(!error.contains("could not build"), "{error}");
        assert!(robot.persistent_agents.lock().await.is_empty());
        assert!(!root.join("build").exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn wifi_preflight_checks_live_device_before_declaring_it_offline() {
    use std::os::unix::fs::PermissionsExt;
    let device = "00008130-000959E43EDA001C";
    let root = std::env::temp_dir().join(format!("ios-wifi-preflight-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let xcrun = root.join("xcrun");
    let cached = json!({"result":{"devices":[{
        "identifier":"CORE-ID", "deviceProperties":{"name":"Phone"},
        "connectionProperties":{"tunnelState":"disconnected","pairingState":"paired",
            "transportType":"localNetwork",
            "potentialHostnames":[format!("{device}.coredevice.local")]}
    }]}});
    let live = json!({"info":{"outcome":"success"},"result":{
        "hardwareProperties":{"udid":device},
        "connectionProperties":{"tunnelState":"connected","pairingState":"paired"}
    }});
    let script = format!(
        "#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\ncase \"$*\" in\n  'devicectl list devices '*) printf '%s' '{}' > \"$last\" ;;\n  'devicectl device info details '*) printf '%s' '{}' > \"$last\" ;;\n  *) exit 1 ;;\nesac\n",
        cached, live
    );
    std::fs::write(&xcrun, script).unwrap();
    std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
    let xcodebuild = root.join("xcodebuild");
    std::fs::write(&xcodebuild, format!(
        "#!/bin/sh\nif [ \"$1\" = '-showdestinations' ]; then\n  printf '%s\\n' 'Available destinations for scheme X:' '{{ platform:iOS, id:{device}, name:Phone }}'\nelse\n  echo 'wifi-helper-build-reached' >&2\n  exit 1\nfi\n"
    )).unwrap();
    std::fs::set_permissions(&xcodebuild, std::fs::Permissions::from_mode(0o700)).unwrap();
    let project = root.join("runner/MobileMcpRealRunner.xcodeproj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("project.pbxproj"),
        "DEVELOPMENT_TEAM = OLDTEAM123;",
    )
    .unwrap();
    std::fs::write(root.join("runner/configure_signing.py"),
        "import pathlib,sys\np=pathlib.Path(sys.argv[2])/'project.pbxproj'\np.write_text(p.read_text().replace('OLDTEAM123','NEWTEAM123'))\nprint('NEWTEAM123')\n"
    ).unwrap();
    std::fs::write(
        root.join("runner/configure_ios_signing.sh"),
        "#!/bin/sh\nexec python3 \"$(dirname \"$0\")/configure_signing.py\" \"$@\"\n",
    )
    .unwrap();
    let launcher = XCTestAgentLauncher::new(
        xcodebuild.to_string_lossy(),
        project,
        root.join("build"),
        "192.168.1.2",
        18787,
        Some("test-token".into()),
    )
    .unwrap();
    let robot = IosXCTestRobot::with_artifact_dir(
        xcrun.to_string_lossy(),
        XCTestAgentBridge::new(None),
        root.join("artifacts"),
    )
    .with_agent_launcher(launcher);
    let report = robot.connection_report(device, true).await.unwrap();
    assert!(
        report.connected,
        "reachable Wi-Fi device was rejected: {}",
        report.summary()
    );
    assert!(report.can_install(), "{}", report.summary());
    assert!(
        !report
            .checks
            .iter()
            .any(|c| c.name == "USB" && c.status != "pass")
    );
    let status = robot.check_device_health(device).await.unwrap();
    assert_ne!(status.state, "offline");
    assert_eq!(status.diagnostics.unwrap()["connected"], true);
    // A fake build failure proves Install passed connection preflight without touching a phone.
    let error = robot
        .prepare_helper(device, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("wifi-helper-build-reached"), "{error}");
    assert_eq!(
        std::fs::read_to_string(root.join("runner/MobileMcpRealRunner.xcodeproj/project.pbxproj"))
            .unwrap(),
        "DEVELOPMENT_TEAM = NEWTEAM123;"
    );
    // Xcode writes the actionable signing error to stdout and may put only
    // TEST BUILD FAILED on stderr. Both lines must reach the classifier.
    std::fs::write(&xcodebuild, format!(
        "#!/bin/sh\nif [ \"$1\" = '-showdestinations' ]; then\n  printf '%s\\n' 'Available destinations for scheme X:' '{{ platform:iOS, id:{device}, name:Phone }}'\nelse\n  printf '%s\\n' 'error: No Account for Team \"NEWTEAM123\".' 'error: No profiles for the app were found.'\n  echo 'TEST BUILD FAILED' >&2\n  exit 1\nfi\n"
    )).unwrap();
    let status = robot.prepare_helper(device, None).await.unwrap();
    assert_eq!(status.state, "signing_account_required");
    assert!(status.detail.contains("NEWTEAM123"));
    assert!(status.detail.contains("Apple Accounts"));
    let polled = robot.check_device_health(device).await.unwrap();
    assert_eq!(polled.state, status.state);
    std::fs::remove_dir_all(root).unwrap();
}

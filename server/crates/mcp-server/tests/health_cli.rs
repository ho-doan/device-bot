use std::process::Command;

#[test]
fn health_requires_exactly_one_device_and_does_not_start_the_server() {
    for arguments in [
        vec!["--health"],
        vec!["--health", "device", "extra"],
        vec!["--health", "   "],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_device-bot"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(
            error.contains("--health requires exactly one Simulator UDID"),
            "{error}"
        );
    }
}

#[cfg(unix)]
#[test]
fn shutdown_simulator_returns_json_and_failure_without_starting_services() {
    use std::os::unix::fs::PermissionsExt;
    let directory =
        std::env::temp_dir().join(format!("device-bot-health-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("xcrun");
    std::fs::write(&script, r##"#!/bin/sh
if [ "$*" != 'simctl list devices available -j' ]; then
    echo "unexpected mutating command: $*" >&2
    exit 99
fi
cat <<'JSON'
{"devices":{"runtime":[{"udid":"8F30E506-B952-48EB-A1DB-8D61701356E0","name":"Test iPhone","state":"Shutdown","isAvailable":true}]}}
JSON
"##).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .args(["--health", "8F30E506-B952-48EB-A1DB-8D61701356E0"])
        .env("XCRUN_PATH", &script)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(!result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["scope"], "ios_simulator");
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["readyForNewSession"], false);
    assert_eq!(report["checks"][1]["name"], "device_booted");
    assert_eq!(report["checks"][1]["status"], "fail");
    assert_eq!(report["recommendedActions"][0], "boot_target_simulator");
}

#[cfg(unix)]
#[test]
fn display_probe_uses_and_removes_a_temporary_png() {
    use std::os::unix::fs::PermissionsExt;
    let directory =
        std::env::temp_dir().join(format!("device-bot-health-display-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("xcrun");
    std::fs::write(&script, r##"#!/bin/sh
case "$*" in
  'simctl list devices available -j')
    echo '{"devices":{"runtime":[{"udid":"8F30E506-B952-48EB-A1DB-8D61701356E0","name":"Test iPhone","state":"Booted","isAvailable":true}]}}' ;;
  'simctl spawn 8F30E506-B952-48EB-A1DB-8D61701356E0 /usr/bin/true') exit 0 ;;
  'simctl io 8F30E506-B952-48EB-A1DB-8D61701356E0 screenshot --type=png '*'/screen.png')
    printf '%s' "$6" > "$HEALTH_PROBE_PATH_FILE"
    printf '\211PNG\r\n\032\n' > "$6" ;;
  *) echo "unexpected command: $*" >&2; exit 99 ;;
esac
"##).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe_path_file = directory.join("probe-path");
    let result = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .args(["--health", "8F30E506-B952-48EB-A1DB-8D61701356E0"])
        .env("XCRUN_PATH", &script)
        .env("HEALTH_PROBE_PATH_FILE", &probe_path_file)
        // Keep this test independent of any developer's active helper bridge.
        .env("DEVICE_BOT_IOS_SIM_AGENT_BIND", "invalid-address")
        .current_dir(&directory)
        .output()
        .unwrap();
    assert!(!directory.join("-").exists());
    let probe_path = std::fs::read_to_string(probe_path_file).unwrap();
    assert!(!std::path::Path::new(&probe_path).exists());
    assert!(!std::path::Path::new(&probe_path).parent().unwrap().exists());
    std::fs::remove_dir_all(&directory).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let display = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "display_response")
        .unwrap();
    assert_eq!(display["status"], "pass", "{report}");
}

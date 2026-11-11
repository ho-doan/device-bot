use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn explicit_mcp_serves_stdio_without_launcher_output() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .arg("mcp")
        .env("DEVICE_BOT_IOS_AGENT_BIND", "127.0.0.1:0")
        .env("DEVICE_BOT_IOS_SIM_AGENT_BIND", "127.0.0.1:0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"cli-test","version":"1"}}}}}}"#).unwrap();
    writeln!(
        input,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    writeln!(input, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert!(
        responses[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "mobile_list_available_devices")
    );
}

#[test]
fn short_flags_print_version_help_and_reject_extra_upgrade_arguments() {
    let version = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .arg("-v")
        .output()
        .unwrap();
    assert!(version.status.success());
    let version = String::from_utf8(version.stdout).unwrap();
    assert!(
        version.trim().chars().next().is_some_and(|c| c.is_ascii_digit()),
        "{version}"
    );

    let help = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .arg("-h")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("-v, --version"), "{help}");
    assert!(help.contains("-u, --upgrade"), "{help}");

    let invalid = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .args(["-u", "extra"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("--upgrade accepts no arguments"));
}

#[test]
fn help_and_invalid_start_do_not_start_services() {
    let help = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in [
        "device-bot mcp",
        "device-bot run",
        "device-bot start",
        "8765",
    ] {
        assert!(help.contains(command), "{help}");
    }
    let invalid = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .args(["start", "extra"])
        .env("DEVICE_BOT_REPORT_DATA_DIR", "")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("start accepts no arguments"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .args(["mcp", "--listen", "invalid-address"])
        .env("DEVICE_BOT_IOS_AGENT_BIND", "invalid-agent-address")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid listen address"));
}

#[test]
fn signing_command_runs_the_shared_script_without_starting_services() {
    let root = std::env::temp_dir().join(format!("signing-cli-{}", uuid::Uuid::new_v4()));
    let runner = root.join("ios-real/xctest-runner");
    let project = runner.join("MobileMcpRealRunner.xcodeproj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("project.pbxproj"), "OLDTEAM123").unwrap();
    std::fs::write(runner.join("configure_ios_signing.sh"),
        "#!/bin/sh\nprintf '%s' 'TEAM123456' > \"$2/project.pbxproj\"\nprintf '%s\\n' 'TEAM123456'\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_device-bot"))
        .arg("--configure-ios-signing")
        .env("DEVICE_BOT_ASSET_DIR", &root)
        .env("DEVICE_BOT_IOS_AGENT_BIND", "invalid-if-service-started")
        .env_remove("DEVICE_BOT_IOS_TEAM")
        .env_remove("DEVICE_BOT_IOS_SIGNING_IDENTITY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["teamId"], "TEAM123456");
    assert_eq!(result["configured"], true);
    assert_eq!(
        std::fs::read_to_string(project.join("project.pbxproj")).unwrap(),
        "TEAM123456"
    );
    std::fs::remove_dir_all(root).unwrap();
}

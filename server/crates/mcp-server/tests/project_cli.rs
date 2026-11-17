//! Installed-suite integration: build device-bot and robot-flow before this test.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct ProjectDir(PathBuf);
impl ProjectDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("device bot project {}", uuid::Uuid::new_v4()));
        let output = Command::new(env!("CARGO_BIN_EXE_device-bot"))
            .arg("init")
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self(path)
    }
    fn run(&self, args: &[&str], env_override: Option<(&str, &str)>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_device-bot"));
        command
            .arg("run")
            .arg(&self.0)
            .args(args)
            .env_remove("DEVICE_BOT_BIN")
            .env_remove("DEVICE_BOT_IOS_AGENT_HOSTS")
            .env_remove("DEVICE_BOT_IOS_AGENT_BIND")
            .env_remove("DEVICE_BOT_IOS_SIM_AGENT_BIND")
            .env_remove("PROJECT_TEST_TEXT")
            .env("DEVICE_BOT_FLUTTER_AUTOSTART", "0")
            .env("DEVICE_BOT_REPORT_DATA_DIR", self.0.join("reports"))
            .current_dir(std::env::temp_dir());
        if let Some((key, value)) = env_override {
            command.env(key, value);
        }
        command.output().unwrap()
    }
}
impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reports(path: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == "report.json") {
                found.push(path);
            } else if path.is_dir() {
                found.extend(reports(&path));
            }
        }
    }
    found
}

#[test]
fn initialized_project_runs_via_real_cli_runner_and_mcp() {
    let project = ProjectDir::new();
    let output = project.run(&[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_paths = reports(&project.0.join("artifacts"));
    assert_eq!(report_paths.len(), 1);
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_paths[0]).unwrap()).unwrap();
    assert_eq!(report["status"], "PASS");
    assert_eq!(report["dry_run"], false);
}

#[test]
fn project_env_is_read_and_process_values_override_it() {
    let project = ProjectDir::new();
    fs::write(
        project.0.join(".env"),
        "DEVICE_BOT_IOS_AGENT_BIND=invalid-from-file\nDEVICE_BOT_IOS_SIM_AGENT_BIND=127.0.0.1:0\n",
    )
    .unwrap();
    // Observe the real backend behavior instead of expecting secret text in a
    // deliberately redacted execution snapshot.
    let output = project.run(&[], Some(("DEVICE_BOT_IOS_AGENT_BIND", "127.0.0.1:0")));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = project.run(&[], None);
    assert!(
        !output.status.success(),
        "invalid env file value must reach the backend"
    );
    fs::write(
        project.0.join(".env"),
        "DEVICE_BOT_IOS_AGENT_BIND=127.0.0.1:0\nDEVICE_BOT_IOS_SIM_AGENT_BIND=127.0.0.1:0\n",
    )
    .unwrap();
    let output = project.run(&[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn project_stops_on_failed_flow_and_returns_nonzero() {
    let project = ProjectDir::new();
    fs::write(
        project.0.join("project.yaml"),
        "version: 1\nkind: project\nname: failure\nflows: [flows/bad.yaml, flows/smoke.yaml]\n",
    )
    .unwrap();
    fs::write(
        project.0.join("flows/bad.yaml"),
        "version: 99\nname: bad\nsteps: []\n",
    )
    .unwrap();
    let output = project.run(&["--dry-run"], None);
    assert!(!output.status.success());
    assert_eq!(reports(&project.0.join("artifacts")).len(), 1);
}

#[test]
fn native_report_helper_runs_without_services_and_preserves_existing_report() {
    let project = ProjectDir::new();
    let output_dir = project.0.join("ci-error");
    let report = output_dir.join("setup/junit.xml");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_device-bot"))
            .args(["project", "ensure-report"])
            .arg(&output_dir)
            .env("DEVICE_BOT_IOS_AGENT_BIND", "invalid-if-services-start")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success(), "{output:?}");
    assert!(
        fs::read_to_string(&report)
            .unwrap()
            .contains("errors=\"1\"")
    );
    fs::write(&report, "preserved real report").unwrap();
    assert!(run().status.success());
    assert_eq!(fs::read_to_string(report).unwrap(), "preserved real report");
}

#[cfg(not(windows))]
#[test]
fn windows_fixture_rejects_non_windows_host_and_missing_state_stop_is_safe() {
    let project = ProjectDir::new();
    for (action, success) in [("start", false), ("stop", true)] {
        let output = Command::new(env!("CARGO_BIN_EXE_device-bot"))
            .args(["project", "windows-fixture", action, "missing-app.exe"])
            .arg(project.0.join("state.json"))
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{output:?}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn named_project_uses_real_cli_and_persistent_mcp_connections() {
    let project = ProjectDir::new();
    fs::write(
        project.0.join(".env"),
        "DEVICE_BOT_IOS_AGENT_BIND=127.0.0.1:0\nDEVICE_BOT_IOS_SIM_AGENT_BIND=127.0.0.1:0\n",
    )
    .unwrap();
    fs::write(project.0.join("project.yaml"), r#"version: 1
kind: project
name: named-smoke
env_file: .env
hosts:
  - {name: desktop, platform: macos, binding: {kind: local}}
resources:
  - {name: first, kind: macos, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 30000}
  - {name: second, kind: macos, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 30000}
flows:
  - path: flows/smoke.yaml
    bindings: [{target: first}, {target: second}]
"#).unwrap();
    fs::write(
        project.0.join("flows/smoke.yaml"),
        "version: 2\nname: named-smoke\nsteps:\n  - wait: {ms: 0}\n",
    )
    .unwrap();
    let output = project.run(&[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = reports(&project.0.join("artifacts"));
    assert_eq!(paths.len(), 2);
    let store = robot_report::Store::open(project.0.join("reports")).unwrap();
    assert!(
        !store.projects().unwrap().is_empty(),
        "named cases must enter report history"
    );
    for path in paths {
        let report: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(report["status"], "PASS");
        assert_eq!(report["dry_run"], false);
    }
}

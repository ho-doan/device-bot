use robot_project::{Project, init};
use std::{fs, path::PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("project-plan-{}", uuid::Uuid::new_v4()));
        init(&path).unwrap();
        fs::write(path.join("profiles.yaml"), "targets: {}\n").unwrap();
        Self(path)
    }
    fn load(&self, body: &str) -> std::io::Result<Project> {
        fs::write(self.0.join("project.yaml"), body).unwrap();
        Project::load(&self.0)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MANIFEST: &str = r#"version: 1
kind: project
name: demo
hosts:
  - name: desktop
    platform: macos
    binding: {kind: local}
  - name: win
    platform: windows
    binding: {kind: mcp, endpoint: '${env:REMOTE_MCP}'}
resources:
  - name: browser
    kind: chrome
    host: desktop
    display: {host: desktop}
    prepare: {kind: launch, url: 'https://example.test'}
    timeout_ms: 30000
    requires: [discovery, input]
  - name: app
    kind: windows
    host: win
    display: {host: desktop, via: remote_desktop}
    selector: stable-window
    prepare: {kind: attach}
    timeout_ms: 5000
flows:
  - path: flows/smoke.yaml
    profiles: profiles.yaml
    bindings:
      - {target: browser, profile: web}
      - {target: app, profile: desktop}
"#;

#[test]
fn plans_shared_flow_and_distinct_execution_display_hosts() {
    let temp = Temp::new();
    let project = temp.load(MANIFEST).unwrap();
    assert!(!project.plan.is_legacy());
    assert_eq!(project.plan.flows[0].bindings.len(), 2);
    assert_eq!(project.plan.resources[1].display.host, "desktop");
    assert_eq!(project.flows[0], project.root.join("flows/smoke.yaml"));
    assert!(!format!("{project:?}").contains("REMOTE_MCP"));
}

#[test]
fn resolves_scoped_values_without_exposing_secrets() {
    let temp = Temp::new();
    let key = format!("PROJECT_SECRET_{}", uuid::Uuid::new_v4().simple());
    fs::write(temp.0.join(".env"), format!("{key}=secret-value\n")).unwrap();
    let source = MANIFEST
        .replace("name: demo", "name: demo\nenv_file: .env")
        .replace("REMOTE_MCP", &key)
        .replace("https://example.test", &format!("${{env:{key}}}"));
    let project = temp.load(&source).unwrap();
    assert_eq!(
        project.resolve(&format!("${{env:{key}}}")).unwrap(),
        "secret-value"
    );
    assert!(std::env::var_os(&key).is_none());
    assert!(!format!("{project:?}").contains("secret-value"));
}

#[test]
fn rejects_invalid_names_references_platforms_and_future_fields() {
    let temp = Temp::new();
    for bad in [
        MANIFEST.replace("name: win", "name: desktop"),
        MANIFEST.replace("name: app", "name: browser"),
        MANIFEST.replace("name: app", "name: '  '"),
        MANIFEST.replace("target: app", "target: missing"),
        MANIFEST.replace("host: win", "host: missing"),
        MANIFEST.replace("host: desktop, via: remote_desktop", "host: desktop"),
        MANIFEST.replace("kind: windows", "kind: ios_simulator"),
        MANIFEST.replace("timeout_ms: 5000", "timeout_ms: 0"),
        MANIFEST.replace("kind: attach", "kind: attach, future: true"),
        MANIFEST.replace(
            "target: app, profile: desktop",
            "target: browser, profile: desktop",
        ),
        MANIFEST.replace(
            "requires: [discovery, input]",
            "requires: [future_capability]",
        ),
    ] {
        assert!(temp.load(&bad).is_err(), "accepted invalid manifest: {bad}");
    }
}

#[tokio::test]
async fn named_plan_is_rejected_before_running_or_creating_artifacts() {
    let temp = Temp::new();
    let project = temp.load(MANIFEST).unwrap();
    let error = robot_project::run(&project, &[], &temp.0, &temp.0, &temp.0)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not supported"));
    assert!(!temp.0.join("artifacts").exists());
}

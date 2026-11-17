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
    let windows_resource = &project.plan.resources[1];
    assert_eq!(windows_resource.host, "win");
    assert_eq!(windows_resource.display.host, "desktop");
    let windows_host = project
        .plan
        .hosts
        .iter()
        .find(|host| host.name == windows_resource.host)
        .unwrap();
    assert_eq!(
        windows_host.platform,
        robot_project::plan::HostPlatform::Windows
    );
    assert!(matches!(
        &windows_host.binding,
        robot_project::plan::HostBinding::Mcp { endpoint, .. }
            if endpoint == "${env:REMOTE_MCP}"
    ));
    assert_eq!(project.plan.resources[1].display.host, "desktop");
    assert_eq!(project.flows[0], project.root.join("flows/smoke.yaml"));
    assert!(!format!("{project:?}").contains("REMOTE_MCP"));
}

#[test]
fn accepts_named_mobile_web_windows_and_desktop_resources() {
    use robot_project::plan::ResourceKind;

    let temp = Temp::new();
    let manifest = r#"version: 1
kind: project
name: all-targets
hosts:
  - name: mac
    platform: macos
    binding: {kind: local}
  - name: win
    platform: windows
    binding: {kind: mcp, endpoint: 'http://windows-host'}
resources:
  - {name: android, kind: android, host: mac, display: {host: mac}, prepare: {kind: attach}, timeout_ms: 5000}
  - {name: ios-simulator, kind: ios_simulator, host: mac, display: {host: mac}, prepare: {kind: attach}, timeout_ms: 5000}
  - {name: ios-device, kind: ios_real, host: mac, display: {host: mac}, prepare: {kind: attach}, timeout_ms: 5000}
  - {name: web, kind: chrome, host: mac, display: {host: mac}, prepare: {kind: attach}, timeout_ms: 5000}
  - {name: windows, kind: windows, host: win, display: {host: mac, via: remote_desktop}, prepare: {kind: attach}, timeout_ms: 5000}
  - {name: desktop, kind: macos, host: mac, display: {host: mac}, prepare: {kind: attach}, timeout_ms: 5000}
flows:
  - path: flows/smoke.yaml
    bindings:
      - {target: android}
      - {target: ios-simulator}
      - {target: ios-device}
      - {target: web}
      - {target: windows}
      - {target: desktop}
"#;

    let project = temp.load(manifest).unwrap();

    assert_eq!(project.plan.hosts.len(), 2);
    assert_eq!(project.plan.resources.len(), 6);
    assert_eq!(project.plan.resources[0].kind, ResourceKind::Android);
    assert_eq!(project.plan.resources[1].kind, ResourceKind::IosSimulator);
    assert_eq!(project.plan.resources[2].kind, ResourceKind::IosReal);
    assert_eq!(project.plan.resources[3].kind, ResourceKind::Chrome);
    assert_eq!(project.plan.resources[4].kind, ResourceKind::Windows);
    assert_eq!(project.plan.resources[5].kind, ResourceKind::Macos);
    assert_eq!(project.plan.flows[0].bindings.len(), 6);
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
async fn unprepared_remote_display_is_rejected_before_connecting() {
    let temp = Temp::new();
    fs::write(
        temp.0.join("profiles.yaml"),
        "version: 1\ntargets: {web: {}, desktop: {}}\n",
    )
    .unwrap();
    let project = temp.load(MANIFEST).unwrap();
    let error = robot_project::run(&project, &[], &temp.0, &temp.0, &temp.0)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("remote display"), "{error}");
}

#[test]
fn rejects_identifiers_that_can_escape_or_collide_on_supported_platforms() {
    let temp = Temp::new();
    let sentinel = temp.0.join("outside-marker");
    fs::write(&sentinel, "untouched").unwrap();
    for value in [
        "../outside-marker",
        "..\\outside-marker",
        "/absolute",
        "C:\\escape",
        ".",
        "..",
        "folder/name",
        "folder\\name",
        "CON",
        "aux",
        "Com1",
        "LPT9",
        "name.",
        "name ",
        "-leading",
        "é",
        &"a".repeat(65),
    ] {
        let quoted = serde_json::to_string(value).unwrap();
        for (from, kind) in [
            ("name: browser", "resource"),
            ("name: desktop", "host"),
            ("target: browser", "flow target"),
        ] {
            let field = from.split(':').next().unwrap();
            let manifest = MANIFEST.replacen(from, &format!("{field}: {quoted}"), 1);
            let error = temp
                .load(&manifest)
                .err()
                .expect("unsafe identifier accepted");
            assert!(
                error.to_string().contains(&format!("{kind} name")),
                "{value:?}: {error}"
            );
        }
    }
    let host_collision = MANIFEST.replace("name: win", "name: DESKTOP");
    assert!(
        temp.load(&host_collision)
            .unwrap_err()
            .to_string()
            .contains("duplicate host name")
    );
    let case_collision = MANIFEST.replace("name: app", "name: BROWSER");
    assert!(
        temp.load(&case_collision)
            .unwrap_err()
            .to_string()
            .contains("duplicate resource name")
    );
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
    assert!(!temp.0.join("artifacts").exists());
}

#[test]
fn accepts_portable_identifier_boundaries() {
    let temp = Temp::new();
    for value in ["a", "desktop_1", "browser-2", &"a".repeat(64)] {
        let manifest = MANIFEST
            .replace("name: browser", &format!("name: {value}"))
            .replace("target: browser", &format!("target: {value}"));
        temp.load(&manifest).unwrap();
    }
}

use async_trait::async_trait;
use robot_flow::mcp::{Mcp, Result as McpResult};
use robot_project::{
    Project,
    plan::{Host, Resource, ResourceKind},
    session::{SessionFactory, run},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("project-actor-tabs-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, content: &str) {
        fs::write(self.0.join(name), content).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    resource: String,
    name: String,
    device_id: Option<String>,
    url: Option<String>,
}

#[derive(Clone, Default)]
struct Factory {
    calls: Arc<Mutex<Vec<Call>>>,
}

struct Fake {
    resource: String,
    device_id: String,
    calls: Arc<Mutex<Vec<Call>>>,
}

#[async_trait]
impl SessionFactory for Factory {
    async fn connect(&self, _: &Project, _: &Host, resource: &Resource) -> McpResult<Box<dyn Mcp>> {
        assert_eq!(resource.kind, ResourceKind::Chrome);
        Ok(Box::new(Fake {
            resource: resource.name.clone(),
            // A transient CDP target ID is returned by the fake driver. It must
            // stay behind the session router and out of flow YAML bindings.
            device_id: format!("chrome:transient-{}", resource.name),
            calls: Arc::clone(&self.calls),
        }))
    }
}

#[async_trait]
impl Mcp for Fake {
    async fn request(&mut self, method: &str, params: Value) -> McpResult<Value> {
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => {
                let names = [
                    "mobile_list_available_devices",
                    "mobile_boot_device",
                    "mobile_open_url",
                    "mobile_get_capabilities",
                    "mobile_click_on_screen_at_coordinates",
                ];
                Ok(
                    json!({"tools":names.iter().map(|name| json!({"name":name,"inputSchema":{"type":"object"}})).collect::<Vec<_>>() }),
                )
            }
            "tools/call" => {
                let name = params["name"].as_str().ok_or("tool name missing")?;
                let args = &params["arguments"];
                let device_id = args
                    .get("deviceId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                self.calls.lock().unwrap().push(Call {
                    resource: self.resource.clone(),
                    name: name.to_owned(),
                    device_id,
                    url: args
                        .get("url")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
                match name {
                    "mobile_list_available_devices" => Ok(json!({
                        "structuredContent": {"devices": [{
                            "id": self.device_id,
                            "name": self.resource,
                            "platform": "chrome",
                            "state": "available"
                        }]}
                    })),
                    "mobile_get_capabilities" => Ok(json!({
                        "structuredContent": {
                            "hostOs": "macos",
                            "deviceId": self.device_id,
                            "capabilities": {
                                "discovery": {"status": "available"},
                                "input": {"status": "available"}
                            }
                        }
                    })),
                    _ => Ok(json!({"content": []})),
                }
            }
            _ => Err(format!("unexpected MCP method {method}").into()),
        }
    }

    async fn notify(&mut self, _: &str, _: Value) -> McpResult<()> {
        Ok(())
    }

    async fn close(&mut self) -> McpResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn actor_bindings_and_step_scopes_route_to_dedicated_chrome_sessions() {
    let temp = Temp::new();
    temp.write(
        "project.yaml",
        "version: 1\nkind: project\nname: actor-tabs\nhosts:\n  - {name: desktop, platform: macos, binding: {kind: local}}\nresources:\n  - {name: chrome-alice, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: launch, url: 'https://example.test/alice'}, timeout_ms: 1000}\n  - {name: chrome-bob, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: launch, url: 'https://example.test/bob'}, timeout_ms: 1000}\nactors:\n  - {name: alice, resource: chrome-alice}\n  - {name: bob, resource: chrome-bob}\nflows:\n  - path: flow.yaml\n    bindings: [{actor: alice}, {actor: bob}]\n",
    );
    temp.write(
        "flow.yaml",
        "version: 2\nname: actor-tabs\nsteps:\n  - id: binding-default\n    tap: {x: 0, y: 0}\n  - id: alice-first\n    resource: alice\n    tap: {x: 1, y: 2}\n  - id: bob\n    resource: bob\n    tap: {x: 3, y: 4}\n  - id: alice-last\n    resource: alice\n    tap: {x: 5, y: 6}\n",
    );

    let yaml = fs::read_to_string(temp.0.join("project.yaml")).unwrap()
        + &fs::read_to_string(temp.0.join("flow.yaml")).unwrap();
    assert!(!yaml.contains("chrome:transient-"));

    let project = Project::load(&temp.0).unwrap();
    let factory = Factory::default();
    let result = run(
        &project,
        &factory,
        &temp.0.join("artifacts"),
        &[],
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(result.reports.len(), 2);
    assert!(result.reports.iter().all(|report| report.status == "PASS"));

    let calls = factory.calls.lock().unwrap();
    for (actor, resource, url) in [
        ("alice", "chrome-alice", "https://example.test/alice"),
        ("bob", "chrome-bob", "https://example.test/bob"),
    ] {
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.resource == resource
                    && call.name == "mobile_open_url"
                    && call.url.as_deref() == Some(url))
                .count(),
            1,
            "each actor's configured URL is opened by its owned Chrome session"
        );
        let clicks: Vec<_> = calls
            .iter()
            .filter(|call| {
                call.resource == resource && call.name == "mobile_click_on_screen_at_coordinates"
            })
            .collect();
        let expected_clicks = if actor == "alice" { 5 } else { 3 };
        assert_eq!(
            clicks.len(),
            expected_clicks,
            "actor scope {actor} should receive only its scoped steps"
        );
        assert!(clicks.iter().all(|call| {
            call.device_id.as_deref() == Some(&format!("chrome:transient-{resource}"))
        }));
    }
}

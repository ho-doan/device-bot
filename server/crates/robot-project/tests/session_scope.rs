use async_trait::async_trait;
use robot_flow::mcp::{Mcp, Result as McpResult};
use robot_project::{
    Project,
    plan::{Host, HostPlatform, Resource, ResourceKind},
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
        let path = std::env::temp_dir().join(format!("project-scope-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, content: &str) {
        fs::write(self.0.join(name), content).unwrap();
    }

    fn project(&self) -> Project {
        Project::load(&self.0).unwrap()
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
}

#[derive(Clone, Default)]
struct Factory {
    calls: Arc<Mutex<Vec<Call>>>,
}

struct Fake {
    resource: String,
    platform: &'static str,
    host_os: &'static str,
    device_id: String,
    calls: Arc<Mutex<Vec<Call>>>,
}

#[async_trait]
impl SessionFactory for Factory {
    async fn connect(
        &self,
        _: &Project,
        host: &Host,
        resource: &Resource,
    ) -> McpResult<Box<dyn Mcp>> {
        let (platform, host_os) = match resource.kind {
            ResourceKind::Chrome => ("chrome", host_os(host.platform)),
            ResourceKind::Macos => ("macos", host_os(host.platform)),
            ResourceKind::Windows => ("windows", host_os(host.platform)),
            _ => return Err("unexpected resource kind in mixed-scope fixture".into()),
        };
        Ok(Box::new(Fake {
            resource: resource.name.clone(),
            platform,
            host_os,
            device_id: format!("{platform}:{}", resource.name),
            calls: Arc::clone(&self.calls),
        }))
    }
}

fn host_os(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Macos => "macos",
        HostPlatform::Windows => "windows",
        HostPlatform::Linux => "linux",
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
                    "mobile_type_keys",
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
                });
                match name {
                    "mobile_list_available_devices" => Ok(json!({
                        "structuredContent": {"devices": [{
                            "id": self.device_id,
                            "name": self.resource,
                            "platform": self.platform,
                            "state": "available"
                        }]}
                    })),
                    "mobile_get_capabilities" => Ok(json!({
                        "structuredContent": {
                            "hostOs": self.host_os,
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

fn write_project(temp: &Temp, host: &str, native_kind: &str) {
    temp.write(
        "project.yaml",
        &format!(
            "version: 1\nkind: project\nname: mixed-scope\nhosts:\n  - name: desktop\n    platform: {host}\n    binding: {{kind: local}}\nresources:\n  - name: browser\n    kind: chrome\n    host: desktop\n    display: {{host: desktop}}\n    prepare: {{kind: launch, url: 'https://example.test/chat'}}\n    timeout_ms: 1000\n  - name: native\n    kind: {native_kind}\n    host: desktop\n    display: {{host: desktop}}\n    prepare: {{kind: attach}}\n    timeout_ms: 1000\nflows:\n  - path: flow.yaml\n    bindings:\n      - {{target: browser}}\n"
        ),
    );
    temp.write(
        "flow.yaml",
        "version: 2\nname: mixed-web-native\nsteps:\n  - id: web-before\n    resource: browser\n    tap: {x: 1, y: 2}\n  - id: native-ui\n    resource: native\n    tap: {x: 3, y: 4}\n  - id: web-after\n    resource: browser\n    tap: {x: 5, y: 6}\n",
    );
}

#[tokio::test]
async fn one_case_routes_chrome_native_and_back_on_macos_and_windows_hosts() {
    for (host, native_kind) in [("macos", "macos"), ("windows", "windows")] {
        let temp = Temp::new();
        write_project(&temp, host, native_kind);
        let project = temp.project();
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

        assert_eq!(result.reports.len(), 1);
        assert_eq!(result.reports[0].status, "PASS");

        let calls = factory.calls.lock().unwrap();
        let clicks: Vec<_> = calls
            .iter()
            .filter(|call| call.name == "mobile_click_on_screen_at_coordinates")
            .cloned()
            .collect();
        assert_eq!(
            clicks,
            [
                Call {
                    resource: "browser".into(),
                    name: "mobile_click_on_screen_at_coordinates".into(),
                    device_id: Some("chrome:browser".into()),
                },
                Call {
                    resource: "native".into(),
                    name: "mobile_click_on_screen_at_coordinates".into(),
                    device_id: Some(format!("{native_kind}:native")),
                },
                Call {
                    resource: "browser".into(),
                    name: "mobile_click_on_screen_at_coordinates".into(),
                    device_id: Some("chrome:browser".into()),
                },
            ],
            "the same compiled case must route web → native → the same Chrome session"
        );
        assert!(calls.iter().any(|call| {
            call.resource == "browser"
                && call.name == "mobile_open_url"
                && call.device_id.as_deref() == Some("chrome:browser")
        }));
    }
}

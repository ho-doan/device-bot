use super::*;
use std::{
    fs,
    sync::{
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(flow: &str, profiles: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("device-project-session-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("project.yaml"),
            r#"
version: 1
kind: project
name: test
hosts:
  - {name: local, platform: macos, binding: {kind: local}}
  - {name: other, platform: macos, binding: {kind: local}}
resources:
  - name: a
    kind: chrome
    host: local
    display: {host: local}
    prepare: {kind: launch, url: 'http://example.test/a'}
    timeout_ms: 1000
  - name: b
    kind: chrome
    host: other
    display: {host: other}
    prepare: {kind: launch, url: 'http://example.test/b'}
    timeout_ms: 1000
flows:
  - path: flow.yaml
    profiles: profiles.yaml
    bindings:
      - {target: a, profile: first}
      - {target: b, profile: second}
"#,
        )
        .unwrap();
        fs::write(root.join("flow.yaml"), flow).unwrap();
        fs::write(root.join("profiles.yaml"), profiles).unwrap();
        Self { root }
    }
    fn load(&self) -> Project {
        Project::load(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[derive(Clone, Default)]
struct Factory {
    events: Arc<StdMutex<Vec<String>>>,
    missing_tool: bool,
    missing_web_proxy: bool,
    fail_boot: bool,
    hang_tap: bool,
    fail_close: bool,
    capability_status: Option<String>,
    host_os: Option<String>,
    discovery_a: Discovery,
}
#[derive(Clone, Copy, Default)]
enum Discovery {
    #[default]
    Ready,
    Delayed,
    Missing,
    StalledAfterEmpty,
    Ambiguous,
    ToolFailure,
}
struct Fake {
    name: String,
    factory: Factory,
    discoveries: usize,
}
#[async_trait]
impl SessionFactory for Factory {
    async fn connect(
        &self,
        _: &Project,
        _: &Host,
        resource: &Resource,
    ) -> mcp::Result<Box<dyn Mcp>> {
        self.events
            .lock()
            .unwrap()
            .push(format!("connect:{}", resource.name));
        Ok(Box::new(Fake {
            name: resource.name.clone(),
            factory: self.clone(),
            discoveries: 0,
        }))
    }
}
#[async_trait]
impl Mcp for Fake {
    async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => {
                let mut names = vec![
                    "mobile_list_available_devices",
                    "mobile_boot_device",
                    "mobile_open_url",
                    "mobile_get_capabilities",
                ];
                if !self.factory.missing_tool {
                    names.push("mobile_click_on_screen_at_coordinates");
                    names.push("mobile_type_keys");
                }
                if !self.factory.missing_web_proxy {
                    names.push(mcp::WEB_TEST_PROXY);
                }
                Ok(
                    json!({"tools": names.iter().map(|name| json!({"name":name,"inputSchema":{"type":"object"}})).collect::<Vec<_>>() }),
                )
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap();
                let id = params["arguments"]["deviceId"].as_str().unwrap_or("");
                self.factory
                    .events
                    .lock()
                    .unwrap()
                    .push(format!("{name}:{}:{id}", self.name));
                if name == "mobile_click_on_screen_at_coordinates" && self.factory.hang_tap {
                    std::future::pending::<()>().await;
                }
                if name == "mobile_boot_device" && self.factory.fail_boot {
                    return Err("boot failed".into());
                }
                if name == mcp::WEB_TEST_PROXY {
                    assert_eq!(params["arguments"]["action"], "configure");
                    assert_eq!(params["arguments"]["config"]["upstream"], "127.0.0.1:8080");
                    return Ok(json!({"structuredContent":{"configured":true}}));
                }
                if name == "mobile_get_capabilities" {
                    return Ok(
                        json!({"structuredContent":{"hostOs":self.factory.host_os.as_deref().unwrap_or("macos"),"deviceId":id,"capabilities":{"discovery":{"status":"available"},"input":{"status":self.factory.capability_status.as_deref().unwrap_or("available")}}}}),
                    );
                }
                if name == "mobile_list_available_devices" {
                    self.discoveries += 1;
                    let mut devices = vec![
                        json!({"id":"chrome:same","name":"page","platform":"chrome","state":"available"}),
                    ];
                    if self.name == "a" {
                        match self.factory.discovery_a {
                                Discovery::Missing => devices.clear(),
                                Discovery::Delayed if self.discoveries <= 2 => devices.clear(),
                                Discovery::StalledAfterEmpty if self.discoveries == 1 => devices.clear(),
                                Discovery::StalledAfterEmpty => return std::future::pending().await,
                                Discovery::Ambiguous => devices.push(
                                    json!({"id":"chrome:other","name":"other page","platform":"chrome","state":"available"}),
                                ),
                                Discovery::ToolFailure => return Ok(json!({"isError":true,"content":[{"text":"private-discovery-detail"}]})),
                                _ => {}
                            }
                    }
                    return Ok(json!({"structuredContent":{"devices":devices}}));
                }
                Ok(json!({"content":[]}))
            }
            _ => Err("unexpected method".into()),
        }
    }
    async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
        Ok(())
    }
    async fn close(&mut self) -> mcp::Result<()> {
        self.factory
            .events
            .lock()
            .unwrap()
            .push(format!("close:{}", self.name));
        if self.factory.fail_close {
            return Err("disconnected during cleanup".into());
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RequestConcurrency {
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    calls: Arc<StdMutex<Vec<(String, String)>>>,
}

struct ConcurrentFake {
    resource: String,
    state: RequestConcurrency,
}

#[async_trait]
impl Mcp for ConcurrentFake {
    async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
        if method != "tools/call" {
            return Err("unexpected method".into());
        }
        let device_id = params["arguments"]["deviceId"]
            .as_str()
            .ok_or("device ID missing")?
            .to_owned();
        let active = self.state.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.maximum.fetch_max(active, Ordering::SeqCst);
        self.state
            .calls
            .lock()
            .unwrap()
            .push((self.resource.clone(), device_id));
        tokio::time::sleep(Duration::from_millis(20)).await;
        self.state.active.fetch_sub(1, Ordering::SeqCst);
        Ok(json!({"content":[]}))
    }

    async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
        Ok(())
    }

    async fn close(&mut self) -> mcp::Result<()> {
        Ok(())
    }
}

const PROFILES: &str = "version: 1\ntargets:\n  first: {}\n  second: {}\n";

mod cleanup;
mod evidence;
mod routing;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use robot_flow::mcp::{self, Mcp};
use robot_project::{
    Project,
    plan::{Host, Resource, ResourceKind},
    session::{self, SessionFactory},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

const VIDEO: &[u8] = include_bytes!("../../robot-flow/tests/fixtures/tiny.mp4");

#[derive(Clone, Default)]
struct Factory {
    events: Arc<Mutex<Vec<String>>>,
    recording_active: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct FakeClient {
    resource: String,
    device_id: String,
    platform: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    recording_active: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

#[async_trait]
impl SessionFactory for Factory {
    async fn connect(
        &self,
        _: &Project,
        host: &Host,
        resource: &Resource,
    ) -> mcp::Result<Box<dyn Mcp>> {
        self.events
            .lock()
            .unwrap()
            .push(format!("connect:{}:{}", resource.name, host.name));

        let (device_id, platform) = match resource.kind {
            ResourceKind::Chrome => ("chrome:page", "chrome"),
            ResourceKind::Macos => ("macos-host", "macos"),
            _ => return Err("unexpected resource kind in recording scope fixture".into()),
        };

        Ok(Box::new(FakeClient {
            resource: resource.name.clone(),
            device_id: device_id.into(),
            platform,
            events: Arc::clone(&self.events),
            recording_active: Arc::clone(&self.recording_active),
            starts: Arc::clone(&self.starts),
            stops: Arc::clone(&self.stops),
        }))
    }
}

fn tool(name: &str) -> Value {
    let properties = match name {
        "mobile_read_screen_recording_chunk" => {
            json!({"recordingId":{"type":"string"},"offset":{"type":"integer"}})
        }
        "mobile_open_url" => {
            json!({"deviceId":{"type":"string"},"url":{"type":"string"}})
        }
        "mobile_click_on_screen_at_coordinates" => json!({
            "deviceId":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"}
        }),
        _ => json!({"deviceId":{"type":"string"}}),
    };
    json!({
        "name": name,
        "inputSchema": {"type":"object", "properties":properties}
    })
}

#[async_trait]
impl Mcp for FakeClient {
    async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => {
                let names: &[&str] = if self.resource == "__project_recorder" {
                    &[
                        "mobile_start_screen_recording",
                        "mobile_stop_screen_recording",
                        "mobile_read_screen_recording_chunk",
                        "mobile_get_capabilities",
                    ]
                } else {
                    &[
                        "mobile_list_available_devices",
                        "mobile_get_capabilities",
                        "mobile_boot_device",
                        "mobile_open_url",
                        "mobile_click_on_screen_at_coordinates",
                    ]
                };
                Ok(json!({
                    "tools": names.iter().map(|name| tool(name)).collect::<Vec<_>>()
                }))
            }
            "tools/call" => {
                let name = params["name"].as_str().ok_or("tool name missing")?;
                let args = &params["arguments"];
                let device = args["deviceId"].as_str().unwrap_or_default();
                match name {
                    "mobile_get_capabilities" if self.resource == "__project_recorder" => {
                        Ok(json!({"structuredContent": {
                            "deviceId": self.device_id,
                            "hostOs": "macos",
                            "capabilities": {"screenRecording":{"status":"available"}}
                        }}))
                    }
                    "mobile_get_capabilities" => Ok(json!({"structuredContent": {
                        "deviceId": self.device_id,
                        "hostOs": "macos",
                        "capabilities": {
                            "discovery":{"status":"available"},
                            "input":{"status":"available"}
                        }
                    }})),
                    "mobile_list_available_devices" => Ok(json!({
                        "structuredContent": {"devices": [{
                            "id": self.device_id,
                            "name": self.resource,
                            "platform": self.platform,
                            "state": "available"
                        }]}
                    })),
                    "mobile_start_screen_recording" => {
                        assert_eq!(self.resource, "__project_recorder");
                        assert_eq!(device, "macos-host");
                        assert!(!self.recording_active.swap(true, Ordering::SeqCst));
                        self.starts.fetch_add(1, Ordering::SeqCst);
                        self.events
                            .lock()
                            .unwrap()
                            .push(format!("recording-start:{device}"));
                        Ok(json!({"content": []}))
                    }
                    "mobile_stop_screen_recording" => {
                        assert_eq!(self.resource, "__project_recorder");
                        assert_eq!(device, "macos-host");
                        assert!(self.recording_active.swap(false, Ordering::SeqCst));
                        self.stops.fetch_add(1, Ordering::SeqCst);
                        self.events
                            .lock()
                            .unwrap()
                            .push(format!("recording-stop:{device}"));
                        Ok(json!({"structuredContent":{"recordingId":"project-recording"}}))
                    }
                    "mobile_read_screen_recording_chunk" => Ok(json!({
                        "structuredContent": {
                            "offset": args["offset"],
                            "size": VIDEO.len(),
                            "data": STANDARD.encode(VIDEO)
                        }
                    })),
                    "mobile_click_on_screen_at_coordinates" => {
                        assert_ne!(self.resource, "__project_recorder");
                        assert!(
                            self.recording_active.load(Ordering::SeqCst),
                            "the desktop recorder must remain active through every scoped target action"
                        );
                        self.events
                            .lock()
                            .unwrap()
                            .push(format!("action:{}:{device}", self.resource));
                        Ok(json!({"content": []}))
                    }
                    _ => {
                        self.events
                            .lock()
                            .unwrap()
                            .push(format!("call:{}:{name}:{device}", self.resource));
                        Ok(json!({"content": []}))
                    }
                }
            }
            _ => Err(format!("unexpected MCP method {method}").into()),
        }
    }

    async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
        Ok(())
    }

    async fn close(&mut self) -> mcp::Result<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("close:{}", self.resource));
        Ok(())
    }
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "device-project-recording-scope-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn project(&self) -> Project {
        std::fs::write(
            self.0.join("project.yaml"),
            "version: 1\nkind: project\nname: recording-scope\nhosts:\n  - {name: desktop, platform: macos, binding: {kind: local}}\nresources:\n  - {name: browser, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: launch, url: 'https://example.test'}, timeout_ms: 5000}\n  - {name: native, kind: macos, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 5000}\nrecording: {host: desktop, start: after_prepare}\nflows:\n  - path: flow.yaml\n    bindings: [{target: browser}]\n",
        )
        .unwrap();
        std::fs::write(
            self.0.join("flow.yaml"),
            "version: 2\nname: web-native-popup\nsteps:\n  - id: web-before-popup\n    resource: browser\n    tap: {x: 1, y: 2}\n  - id: native-permission-popup\n    note: allow the visible native permission dialog\n    resource: native\n    tap: {x: 3, y: 4}\n  - id: web-after-popup\n    resource: browser\n    tap: {x: 5, y: 6}\n",
        )
        .unwrap();
        Project::load(&self.0).unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn one_desktop_recorder_covers_web_native_popup_and_return_to_web() {
    let temp = Temp::new();
    let project = temp.project();
    let factory = Factory::default();

    let result = session::run(
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
    assert_eq!(factory.starts.load(Ordering::SeqCst), 1);
    assert_eq!(factory.stops.load(Ordering::SeqCst), 1);
    assert!(!factory.recording_active.load(Ordering::SeqCst));

    let events = factory.events.lock().unwrap().clone();
    let actions: Vec<_> = events
        .iter()
        .filter(|event| event.starts_with("action:"))
        .cloned()
        .collect();
    assert_eq!(
        actions,
        [
            "action:browser:chrome:page",
            "action:native:macos-host",
            "action:browser:chrome:page",
        ]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| *event == "connect:__project_recorder:desktop")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| *event == "recording-start:macos-host")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| *event == "recording-stop:macos-host")
            .count(),
        1
    );

    let start = events
        .iter()
        .position(|event| event == "recording-start:macos-host")
        .unwrap();
    let first_action = events
        .iter()
        .position(|event| event.starts_with("action:"))
        .unwrap();
    let stop = events
        .iter()
        .position(|event| event == "recording-stop:macos-host")
        .unwrap();
    let last_action = events
        .iter()
        .rposition(|event| event.starts_with("action:"))
        .unwrap();
    assert!(start < first_action);
    assert!(last_action < stop);
}

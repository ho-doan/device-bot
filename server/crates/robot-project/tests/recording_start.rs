use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use robot_flow::mcp::{self, Mcp};
use robot_project::{
    Project,
    plan::{Host, Resource},
    session::{self, SessionFactory},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const VIDEO: &[u8] = include_bytes!("../../robot-flow/tests/fixtures/tiny.mp4");

#[derive(Clone, Default)]
struct Factory {
    events: Arc<Mutex<Vec<String>>>,
}

struct FakeClient {
    resource: String,
    events: Arc<Mutex<Vec<String>>>,
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
        Ok(Box::new(FakeClient {
            resource: resource.name.clone(),
            events: Arc::clone(&self.events),
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
        "mobile_type_keys" => json!({
            "deviceId":{"type":"string"},"text":{"type":"string"}
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
                        "mobile_type_keys",
                    ]
                };
                Ok(json!({"tools":names.iter().map(|name| tool(name)).collect::<Vec<_>>() }))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let device = params["arguments"]["deviceId"].as_str().unwrap_or_default();
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("call:{}:{name}:{device}", self.resource));
                match name {
                    "mobile_get_capabilities" if self.resource == "__project_recorder" => Ok(
                        json!({"structuredContent":{"deviceId":"macos-host","hostOs":"macos","capabilities":{"screenRecording":{"status":"available"}}}}),
                    ),
                    "mobile_get_capabilities" => Ok(json!({
                        "structuredContent": {
                            "deviceId": device,
                            "hostOs": "macos",
                            "capabilities": {"input":{"status":"available"}}
                        }
                    })),
                    "mobile_list_available_devices" => {
                        let (id, platform, name) = match self.resource.as_str() {
                            "browser" => ("chrome:page", "chrome", "browser"),
                            "desktop-ui" => ("macos-host", "macos", "desktop"),
                            _ => return Err("unexpected discovery session".into()),
                        };
                        Ok(json!({"structuredContent":{"devices":[{
                            "id":id,"name":name,"platform":platform,"state":"available"
                        }]}}))
                    }
                    "mobile_stop_screen_recording" if self.resource == "__project_recorder" => {
                        Ok(json!({"structuredContent":{"recordingId":"project-recording"}}))
                    }
                    "mobile_read_screen_recording_chunk"
                        if self.resource == "__project_recorder" =>
                    {
                        Ok(json!({"structuredContent":{
                            "offset":0,
                            "size":VIDEO.len(),
                            "data":STANDARD.encode(VIDEO)
                        }}))
                    }
                    _ => Ok(json!({"content":[]})),
                }
            }
            _ => Err("unexpected MCP method".into()),
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

fn project_fixture(start: &str) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!(
        "device-project-recording-start-{start}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("project.yaml"),
        format!(
            "version: 1\nkind: project\nname: recording-start\nhosts:\n  - {{name: desktop, platform: macos, binding: {{kind: local}}}}\nresources:\n  - {{name: browser, kind: chrome, host: desktop, display: {{host: desktop}}, prepare: {{kind: launch, url: 'https://example.test'}}, timeout_ms: 5000, requires: [input]}}\n  - {{name: desktop-ui, kind: macos, host: desktop, display: {{host: desktop}}, selector: macos-host, prepare: {{kind: attach}}, timeout_ms: 5000, requires: [input]}}\nrecording: {{host: desktop, start: {start}}}\nflows:\n  - path: flow.yaml\n    bindings: [{{target: browser}}]\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("flow.yaml"),
        "version: 2\nname: smoke\nsteps:\n  - tap: {x: 3, y: 4}\n",
    )
    .unwrap();
    let project = Project::load(&root).unwrap();
    (root, project)
}

fn event_index(events: &[String], event: &str) -> usize {
    events
        .iter()
        .position(|candidate| candidate == event)
        .unwrap_or_else(|| panic!("missing event {event}; observed {events:#?}"))
}

#[tokio::test]
async fn before_prepare_starts_recording_before_connecting_or_preparing_targets() {
    let (root, project) = project_fixture("before_prepare");
    let factory = Factory::default();

    session::run(
        &project,
        &factory,
        &root.join("artifacts"),
        &[],
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    let events = factory.events.lock().unwrap().clone();
    let start = event_index(
        &events,
        "call:__project_recorder:mobile_start_screen_recording:macos-host",
    );
    let first_target_connect = event_index(&events, "connect:browser:desktop");
    let open = event_index(&events, "call:browser:mobile_open_url:chrome:page");
    assert!(start < first_target_connect, "{events:#?}");
    assert!(start < open, "{events:#?}");
    assert!(
        events
            .iter()
            .any(|event| { event == "call:desktop-ui:mobile_list_available_devices:" })
    );
    assert!(events.iter().any(|event| {
        event == "call:browser:mobile_click_on_screen_at_coordinates:chrome:page"
    }));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn after_prepare_starts_recording_after_all_targets_open_before_first_flow_action() {
    let (root, project) = project_fixture("after_prepare");
    let factory = Factory::default();

    session::run(
        &project,
        &factory,
        &root.join("artifacts"),
        &[],
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    let events = factory.events.lock().unwrap().clone();
    let native_discovery = event_index(&events, "call:desktop-ui:mobile_list_available_devices:");
    let browser_open = event_index(&events, "call:browser:mobile_open_url:chrome:page");
    let start = event_index(
        &events,
        "call:__project_recorder:mobile_start_screen_recording:macos-host",
    );
    let first_action = event_index(
        &events,
        "call:browser:mobile_click_on_screen_at_coordinates:chrome:page",
    );
    assert!(native_discovery < start, "{events:#?}");
    assert!(browser_open < start, "{events:#?}");
    assert!(start < first_action, "{events:#?}");
    std::fs::remove_dir_all(root).unwrap();
}

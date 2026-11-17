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
        self.events.lock().unwrap().push(format!(
            "connect:{}:{}:{:?}:{}",
            resource.name,
            host.name,
            host.platform,
            resource.selector.as_deref().unwrap_or("")
        ));
        Ok(Box::new(FakeClient {
            resource: resource.name.clone(),
            events: Arc::clone(&self.events),
        }))
    }
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
                        "mobile_click_on_screen_at_coordinates",
                    ]
                };
                let tools = names
                    .iter()
                    .map(|name| {
                        let (properties, required) = match *name {
                            "mobile_read_screen_recording_chunk" => (
                                json!({"recordingId":{"type":"string"},"offset":{"type":"integer"}}),
                                json!(["recordingId", "offset"]),
                            ),
                            "mobile_click_on_screen_at_coordinates" => (
                                json!({"deviceId":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"}}),
                                json!(["deviceId", "x", "y"]),
                            ),
                            _ => (
                                json!({"deviceId":{"type":"string"}}),
                                json!(["deviceId"]),
                            ),
                        };
                        json!({"name":name,"inputSchema":{"type":"object","properties":properties,"required":required}})
                    })
                    .collect::<Vec<_>>();
                Ok(json!({"tools":tools}))
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
                            "capabilities": {}
                        }
                    })),
                    "mobile_list_available_devices" => Ok(json!({
                        "structuredContent": {
                            "devices": [{"id":"chrome:page","name":"browser","platform":"chrome","state":"available"}]
                        }
                    })),
                    "mobile_start_screen_recording" => Ok(json!({"content":[]})),
                    "mobile_stop_screen_recording" => {
                        Ok(json!({"structuredContent":{"recordingId":"desktop-recording"}}))
                    }
                    "mobile_read_screen_recording_chunk" => {
                        let offset = params["arguments"]["offset"].as_u64().unwrap() as usize;
                        let end = (offset + 64 * 1024).min(VIDEO.len());
                        Ok(
                            json!({"structuredContent":{"recordingId":"desktop-recording","offset":offset,"size":VIDEO.len(),"data":STANDARD.encode(&VIDEO[offset..end])}}),
                        )
                    }
                    "mobile_click_on_screen_at_coordinates" => Ok(json!({"content":[]})),
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

fn project_fixture() -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!(
        "device-project-recording-attach-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("project.yaml"),
        "version: 1\nkind: project\nname: recording-attach\nhosts:\n  - {name: desktop, platform: macos, binding: {kind: local}}\nresources:\n  - {name: browser, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 1000}\nrecording: {host: desktop, start: after_prepare}\nflows:\n  - path: flow.yaml\n    bindings: [{target: browser}]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("flow.yaml"),
        "version: 2\nname: smoke\nsteps:\n  - tap: {x: 1, y: 2}\n",
    )
    .unwrap();
    let project = Project::load(&root).unwrap();
    (root, project)
}

#[test]
fn recording_config_rejects_unknown_desktop_host() {
    let (root, _) = project_fixture();
    let path = root.join("project.yaml");
    let manifest = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        manifest.replace(
            "recording: {host: desktop, start: after_prepare}",
            "recording: {host: missing, start: after_prepare}",
        ),
    )
    .unwrap();

    let error = match Project::load(&root) {
        Ok(_) => panic!("recording must reference a declared host"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("recording references unknown host")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_attaches_one_recorder_to_declared_desktop_host_separately_from_flow_target() {
    let (root, project) = project_fixture();
    let factory = Factory::default();

    session::run(
        &project,
        &factory,
        &root.join("artifacts"),
        &[],
        Duration::from_secs(5),
    )
    .await
    .unwrap();

    let events = factory.events.lock().unwrap().clone();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("connect:__project_recorder:"))
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .any(|event| { event == "connect:__project_recorder:desktop:Macos:macos-host" })
    );
    assert!(
        events
            .iter()
            .any(|event| { event == "connect:browser:desktop:Macos:" })
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "close:__project_recorder")
            .count(),
        1
    );
    assert!(events.iter().any(|event| {
        event == "call:__project_recorder:mobile_start_screen_recording:macos-host"
    }));
    assert!(events.iter().any(|event| {
        event == "call:__project_recorder:mobile_stop_screen_recording:macos-host"
    }));
    assert!(
        events.iter().any(|event| {
            event == "call:__project_recorder:mobile_read_screen_recording_chunk:"
        })
    );
    assert!(events.iter().any(|event| {
        event == "call:browser:mobile_click_on_screen_at_coordinates:chrome:page"
    }));
    assert!(!events.iter().any(|event| {
        event.starts_with("call:__project_recorder:mobile_click_on_screen_at_coordinates:")
    }));
    assert_eq!(
        std::fs::read(root.join("artifacts/recording/project-video.mp4")).unwrap(),
        VIDEO
    );
    std::fs::remove_dir_all(root).unwrap();
}

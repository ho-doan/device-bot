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
use tokio::sync::{Notify, watch};

const VIDEO: &[u8] = include_bytes!("../../robot-flow/tests/fixtures/tiny.mp4");

#[derive(Clone)]
struct Factory {
    events: Arc<Mutex<Vec<String>>>,
    action_started: Arc<Notify>,
    fail_action: bool,
    hang_action: bool,
}

impl Default for Factory {
    fn default() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            action_started: Arc::new(Notify::new()),
            fail_action: false,
            hang_action: false,
        }
    }
}

struct FakeClient {
    resource: String,
    events: Arc<Mutex<Vec<String>>>,
    action_started: Arc<Notify>,
    fail_action: bool,
    hang_action: bool,
}

fn tool(name: &str) -> Value {
    let schema = match name {
        "mobile_read_screen_recording_chunk" => json!({
            "type":"object",
            "properties":{"recordingId":{"type":"string"},"offset":{"type":"integer"}},
            "required":["recordingId","offset"]
        }),
        "mobile_click_on_screen_at_coordinates" => json!({
            "type":"object",
            "properties":{"deviceId":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"}},
            "required":["deviceId","x","y"]
        }),
        "mobile_type_keys" => json!({
            "type":"object",
            "properties":{"deviceId":{"type":"string"},"text":{"type":"string"}},
            "required":["deviceId","text"]
        }),
        "mobile_list_available_devices" => json!({"type":"object","properties":{}}),
        _ => json!({
            "type":"object",
            "properties":{"deviceId":{"type":"string"}},
            "required":["deviceId"]
        }),
    };
    json!({"name":name,"inputSchema":schema})
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
            action_started: Arc::clone(&self.action_started),
            fail_action: self.fail_action,
            hang_action: self.hang_action,
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
                    "mobile_list_available_devices" => Ok(json!({
                        "structuredContent": {"devices":[{
                            "id":"chrome:page","name":"browser","platform":"chrome","state":"available"
                        }]}
                    })),
                    "mobile_click_on_screen_at_coordinates" if self.hang_action => {
                        self.action_started.notify_one();
                        std::future::pending::<mcp::Result<Value>>().await
                    }
                    "mobile_click_on_screen_at_coordinates" if self.fail_action => Ok(
                        json!({"isError":true,"content":[{"type":"text","text":"fixture action failed"}]}),
                    ),
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

struct ProjectDir(PathBuf);

impl ProjectDir {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "device-project-recording-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("project.yaml"),
            r#"version: 1
kind: project
name: recording-cleanup
hosts:
  - {name: desktop, platform: macos, binding: {kind: local}}
resources:
  - {name: browser, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 5000, requires: [input]}
recording: {host: desktop, start: before_prepare}
flows:
  - path: flow.yaml
    bindings: [{target: browser}]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("flow.yaml"),
            "version: 2\nname: cleanup\nsteps:\n  - tap: {x: 10, y: 20}\n",
        )
        .unwrap();
        Self(root)
    }

    fn load(&self) -> Project {
        Project::load(&self.0).unwrap()
    }
}

impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn event_count(events: &[String], prefix: &str) -> usize {
    events
        .iter()
        .filter(|event| event.starts_with(prefix))
        .count()
}

fn assert_finalized(factory: &Factory, output: &PathBuf) {
    let events = factory.events.lock().unwrap().clone();
    assert_eq!(
        event_count(
            &events,
            "call:__project_recorder:mobile_start_screen_recording:"
        ),
        1,
        "recording must start exactly once: {events:#?}"
    );
    assert_eq!(
        event_count(
            &events,
            "call:__project_recorder:mobile_stop_screen_recording:"
        ),
        1,
        "recording must stop exactly once: {events:#?}"
    );
    assert_eq!(
        event_count(
            &events,
            "call:__project_recorder:mobile_read_screen_recording_chunk:"
        ),
        1,
        "finalized video must be transferred: {events:#?}"
    );
    assert_eq!(event_count(&events, "close:__project_recorder"), 1);
    assert!(
        output.join("recording/project-video.mp4").is_file(),
        "finalized project video should be published"
    );
}

#[tokio::test]
async fn recording_is_finalized_after_success() {
    let directory = ProjectDir::new();
    let project = directory.load();
    let factory = Factory::default();
    let output = directory.0.join("artifacts");

    let result = session::run(&project, &factory, &output, &[], Duration::from_secs(10))
        .await
        .unwrap();

    assert_eq!(result.reports[0].status, "PASS");
    assert_finalized(&factory, &output);
}

#[tokio::test]
async fn recording_is_finalized_after_flow_failure() {
    let directory = ProjectDir::new();
    let project = directory.load();
    let factory = Factory {
        fail_action: true,
        ..Factory::default()
    };
    let output = directory.0.join("artifacts");

    let result = session::run(&project, &factory, &output, &[], Duration::from_secs(10))
        .await
        .unwrap();

    assert_eq!(result.reports[0].status, "FAIL");
    assert_finalized(&factory, &output);
}

#[tokio::test]
async fn recording_is_finalized_after_explicit_cancellation() {
    let directory = ProjectDir::new();
    let project = directory.load();
    let factory = Factory {
        hang_action: true,
        ..Factory::default()
    };
    let task_factory = factory.clone();
    let output = directory.0.join("artifacts");
    let (cancel, receiver) = watch::channel(false);
    let task_output = output.clone();
    let task = tokio::spawn(async move {
        session::run_with_history_cancel(
            &project,
            &task_factory,
            &task_output,
            &[],
            Duration::from_secs(30),
            None,
            receiver,
        )
        .await
    });

    tokio::time::timeout(Duration::from_secs(5), factory.action_started.notified())
        .await
        .expect("flow action should begin");
    cancel.send(true).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("cancelled project should finish cleanup")
        .unwrap();
    let error = match result {
        Ok(_) => panic!("explicit cancellation should interrupt the project"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), robot_flow::outcome::ErrorKind::Interrupted);
    assert_finalized(&factory, &output);
}

//! Project-owned desktop recording and evidence finalization.
use crate::{
    Project, invalid,
    plan::{DisplayBinding, HostPlatform, Preparation, ProjectPlan, Resource, ResourceKind},
    session::SessionFactory,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use robot_flow::mcp::{self, Mcp};
use serde::Deserialize;
use serde_json::json;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordingConfig {
    pub host: String,
    pub start: StartPhase,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartPhase {
    BeforePrepare,
    AfterPrepare,
}

pub(crate) fn validate_config(
    config: Option<&RecordingConfig>,
    plan: &ProjectPlan,
) -> io::Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    let host = plan
        .hosts
        .iter()
        .find(|h| h.name == config.host)
        .ok_or_else(|| invalid("recording references unknown host"))?;
    if host.platform == HostPlatform::Linux {
        return Err(invalid("Linux desktop recording is unsupported"));
    }
    if plan.is_legacy() {
        return Err(invalid("project recording requires named resources"));
    }
    if plan.resources.iter().any(|r| r.display.host != config.host) {
        return Err(invalid(
            "every resource must be displayed on the recording host",
        ));
    }
    Ok(())
}

pub(crate) type SharedRecorder = Arc<Mutex<Recorder>>;
pub(crate) struct Recorder {
    client: Option<Box<dyn Mcp>>,
    device: String,
    active: bool,
    directory: PathBuf,
    ffmpeg: Option<PathBuf>,
    origin: Option<Instant>,
    origin_unix_ms: Option<u128>,
    events: Arc<StdMutex<Vec<serde_json::Value>>>,
}
impl Recorder {
    pub fn new(directory: &Path, project: &Project) -> SharedRecorder {
        Arc::new(Mutex::new(Self {
            client: None,
            device: String::new(),
            active: false,
            directory: directory.to_owned(),
            ffmpeg: project
                .environment_value("DEVICE_BOT_FFMPEG_PATH")
                .map(PathBuf::from),
            origin: None,
            origin_unix_ms: None,
            events: Arc::new(StdMutex::new(Vec::new())),
        }))
    }
    pub fn event(&mut self, event: &'static str, case: Option<usize>) {
        if let Some(origin) = self.origin {
            self.events.lock().unwrap().push(
                json!({"offset_ms":origin.elapsed().as_millis(), "event":event, "case":case}),
            );
        }
    }

    pub fn step_sink(
        &self,
        case: usize,
        default_target: String,
    ) -> Option<robot_flow::FlowStepEventSink> {
        let origin = self.origin?;
        let events = Arc::clone(&self.events);
        Some(Arc::new(move |event| {
            let target = event
                .resource
                .as_deref()
                .or(event.platform.as_deref())
                .unwrap_or(&default_target);
            events.lock().unwrap().push(json!({
                "offset_ms": origin.elapsed().as_millis(),
                "event": "step",
                "phase": event.phase.as_str(),
                "case": case,
                "step": event.step_id,
                "action": event.action,
                "target": target,
                "resource": event.resource,
                "platform": event.platform,
            }));
        }))
    }
}

pub(crate) async fn prepare(
    recorder: &SharedRecorder,
    project: &Project,
    factory: &dyn SessionFactory,
) -> mcp::Result<()> {
    let config = project
        .recording
        .as_ref()
        .ok_or("recording configuration missing")?;
    let host = project
        .plan
        .hosts
        .iter()
        .find(|h| h.name == config.host)
        .ok_or("recording host missing")?;
    let (kind, device) = match host.platform {
        HostPlatform::Macos => (ResourceKind::Macos, "macos-host"),
        HostPlatform::Windows => (ResourceKind::Windows, "windows-host"),
        HostPlatform::Linux => return Err("Linux desktop recording unsupported".into()),
    };
    let resource = Resource {
        name: "__project_recorder".into(),
        kind,
        host: host.name.clone(),
        display: DisplayBinding {
            host: host.name.clone(),
            via: None,
        },
        selector: Some(device.into()),
        prepare: Preparation::Attach {},
        timeout_ms: 30_000,
        requires: Vec::new(),
        web_proxy: None,
    };
    let client = tokio::time::timeout(
        Duration::from_secs(30),
        factory.connect(project, host, &resource),
    )
    .await
    .map_err(|_| "recorder connection timed out")??;
    let mut guard = recorder.lock().await;
    guard.client = Some(client);
    guard.device = device.into();
    let client = guard.client.as_mut().unwrap();
    let tools = tokio::time::timeout(Duration::from_secs(30), mcp::initialize(client.as_mut()))
        .await
        .map_err(|_| "recorder initialization timed out")??;
    for name in [
        "mobile_start_screen_recording",
        "mobile_stop_screen_recording",
    ] {
        let schema = tools
            .get(name)
            .ok_or("desktop recording tool unavailable")?;
        mcp::validate_arguments(schema, &json!({"deviceId":device}))?;
    }
    let schema = tools
        .get("mobile_read_screen_recording_chunk")
        .ok_or("session-owned recording transfer unavailable")?;
    mcp::validate_arguments(schema, &json!({"recordingId":"preflight", "offset":0}))?;
    let capability_schema = tools
        .get("mobile_get_capabilities")
        .ok_or("desktop capability tool unavailable")?;
    mcp::validate_arguments(capability_schema, &json!({"deviceId":device}))?;
    let capabilities = tokio::time::timeout(
        Duration::from_secs(30),
        guard.client.as_mut().unwrap().request(
            "tools/call",
            json!({"name":"mobile_get_capabilities","arguments":{"deviceId":device}}),
        ),
    )
    .await
    .map_err(|_| "recorder capability check timed out")??;
    let report = &capabilities["structuredContent"];
    let expected_host = match host.platform {
        HostPlatform::Macos => "macos",
        HostPlatform::Windows => "windows",
        HostPlatform::Linux => "linux",
    };
    if capabilities["isError"] == true
        || report["deviceId"] != device
        || report["hostOs"] != expected_host
        || report["capabilities"]["screenRecording"]["status"] != "available"
    {
        return Err("desktop recording capability unavailable or host identity mismatch".into());
    }
    Ok(())
}

pub(crate) async fn start(recorder: &SharedRecorder) -> mcp::Result<()> {
    let mut guard = recorder.lock().await;
    if guard.active || guard.client.is_none() {
        return Err("recorder is not ready to start".into());
    }
    let device = guard.device.clone();
    guard.active = true;
    guard.origin = Some(Instant::now());
    guard.origin_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|v| v.as_millis());
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        guard.client.as_mut().unwrap().request(
            "tools/call",
            json!({"name":"mobile_start_screen_recording","arguments":{"deviceId":device}}),
        ),
    )
    .await
    .map_err(|_| "desktop recording start timed out")??;
    if result["isError"] == true {
        guard.active = false;
        return Err("desktop recording start failed".into());
    }
    guard.event("recording_started", None);
    Ok(())
}

pub(crate) async fn finish(recorder: &SharedRecorder) -> mcp::Result<()> {
    let recorder = Arc::clone(recorder);
    tokio::spawn(async move { finish_inner(&recorder).await })
        .await
        .map_err(|_| "recorder cleanup task failed")?
}

async fn finish_inner(recorder: &SharedRecorder) -> mcp::Result<()> {
    let mut guard = recorder.lock().await;
    let mut outcome = Ok(());
    if guard.active {
        guard.active = false;
        guard.event("recording_stopping", None);
        let device = guard.device.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(120),
            guard.client.as_mut().unwrap().request(
                "tools/call",
                json!({"name":"mobile_stop_screen_recording","arguments":{"deviceId":device}}),
            ),
        )
        .await;
        outcome = match result {
            Ok(Ok(value)) if value["isError"] != true => {
                match value["structuredContent"]["recordingId"].as_str() {
                    Some(id) => download(&mut guard, id.to_owned()).await,
                    None => Err("session-owned recording transfer unavailable".into()),
                }
            }
            _ => Err("desktop recording stop failed".into()),
        };
        if std::fs::create_dir_all(&guard.directory)
            .and_then(|_| {
                std::fs::write(
                    guard.directory.join("project-timeline.json"),
                    serde_json::to_vec_pretty(&json!({"origin":"start_request", "origin_unix_ms":guard.origin_unix_ms, "alignment":"capture begins between start_request and recording_started acknowledgement", "events":*guard.events.lock().unwrap()})).unwrap(),
                )
            })
            .is_err()
            && outcome.is_ok()
        {
            outcome = Err("cannot save project timeline".into());
        }
    }
    if let Some(mut client) = guard.client.take() {
        if !matches!(
            tokio::time::timeout(Duration::from_secs(10), client.close()).await,
            Ok(Ok(()))
        ) && outcome.is_ok()
        {
            outcome = Err("recorder session cleanup failed".into());
        }
    }
    outcome
}

async fn download(guard: &mut Recorder, id: String) -> mcp::Result<()> {
    use tokio::io::AsyncWriteExt;
    const MAX: u64 = 1024 * 1024 * 1024;
    const CHUNK: usize = 64 * 1024;
    tokio::fs::create_dir_all(&guard.directory)
        .await
        .map_err(|_| "cannot create recording evidence directory")?;
    let temporary = guard.directory.join("project-video.mp4.partial");
    let result: mcp::Result<()> = async {
        let mut output = tokio::fs::File::create(&temporary).await.map_err(|_| "cannot create recording evidence")?;
        let mut offset = 0u64;
        let mut expected = None;
        loop {
            let response = tokio::time::timeout(Duration::from_secs(30), guard.client.as_mut().ok_or("recorder session missing")?.request("tools/call", json!({"name":"mobile_read_screen_recording_chunk","arguments":{"recordingId":id,"offset":offset}}))).await.map_err(|_| "recording transfer timed out")??;
            let chunk = &response["structuredContent"];
            let size = chunk["size"].as_u64().filter(|n| *n > 0 && *n <= MAX).ok_or("invalid recording transfer size")?;
            if response["isError"] == true || chunk["offset"].as_u64() != Some(offset) || expected.is_some_and(|n| n != size) { return Err("invalid recording transfer response".into()); }
            expected = Some(size);
            let bytes = STANDARD.decode(chunk["data"].as_str().ok_or("recording chunk missing")?).map_err(|_| "invalid recording chunk")?;
            if bytes.is_empty() || bytes.len() > CHUNK || bytes.len() as u64 > size - offset { return Err("invalid recording chunk length".into()); }
            output.write_all(&bytes).await.map_err(|_| "cannot save recording chunk")?;
            offset += bytes.len() as u64;
            if offset == size { break; }
        }
        output.flush().await.map_err(|_| "cannot flush recording")?;
        drop(output);
        robot_flow::video_validation::validate_video(&temporary, guard.ffmpeg.as_deref(), Duration::from_secs(120)).await.map_err(|_| "desktop recording video validation failed")?;
        tokio::fs::rename(&temporary, guard.directory.join("project-video.mp4")).await.map_err(|_| "cannot publish project video")?;
        Ok(())
    }.await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Mutex as StdMutex;
    const VIDEO: &[u8] = include_bytes!("../../robot-flow/tests/fixtures/tiny.mp4");
    struct FakeFactory {
        calls: Arc<StdMutex<Vec<String>>>,
        bytes: Vec<u8>,
    }
    struct FakeClient {
        calls: Arc<StdMutex<Vec<String>>>,
        bytes: Vec<u8>,
    }
    #[async_trait]
    impl SessionFactory for FakeFactory {
        async fn connect(
            &self,
            _: &Project,
            _: &crate::plan::Host,
            _: &Resource,
        ) -> mcp::Result<Box<dyn Mcp>> {
            Ok(Box::new(FakeClient {
                calls: Arc::clone(&self.calls),
                bytes: self.bytes.clone(),
            }))
        }
    }
    #[async_trait]
    impl Mcp for FakeClient {
        async fn request(&mut self, method: &str, args: Value) -> mcp::Result<Value> {
            match method {
                "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
                "tools/list" => {
                    let tools = [
                        "mobile_start_screen_recording",
                        "mobile_stop_screen_recording",
                        "mobile_read_screen_recording_chunk",
                        "mobile_get_capabilities",
                    ]
                    .iter()
                    .map(|name| json!({"name":name,"inputSchema":{"type":"object"}}))
                    .collect::<Vec<_>>();
                    Ok(json!({"tools":tools}))
                }
                "tools/call" => {
                    let name = args["name"].as_str().unwrap();
                    self.calls.lock().unwrap().push(name.into());
                    match name {
                        "mobile_get_capabilities" => Ok(
                            json!({"structuredContent":{"hostOs":"macos","deviceId":"macos-host","capabilities":{"screenRecording":{"status":"available"}}}}),
                        ),
                        "mobile_start_screen_recording" => Ok(json!({})),
                        "mobile_stop_screen_recording" => {
                            Ok(json!({"structuredContent":{"recordingId":"opaque"}}))
                        }
                        "mobile_read_screen_recording_chunk" => {
                            let offset = args["arguments"]["offset"].as_u64().unwrap() as usize;
                            let end = (offset + 65536).min(self.bytes.len());
                            Ok(
                                json!({"structuredContent":{"offset":offset,"size":self.bytes.len(),"data":STANDARD.encode(&self.bytes[offset..end])}}),
                            )
                        }
                        _ => Err("unexpected tool".into()),
                    }
                }
                _ => Err("unexpected method".into()),
            }
        }
        async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
            Ok(())
        }
        async fn close(&mut self) -> mcp::Result<()> {
            self.calls.lock().unwrap().push("close".into());
            Ok(())
        }
    }
    fn fixture() -> (PathBuf, Project) {
        let root =
            std::env::temp_dir().join(format!("project-recording-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("flow.yaml"),
            "version: 2\nname: test\nsteps:\n  - wait: {ms: 0}\n",
        )
        .unwrap();
        std::fs::write(root.join("project.yaml"), "version: 1\nkind: project\nname: test\nhosts:\n  - {name: desktop, platform: macos, binding: {kind: local}}\nresources:\n  - {name: browser, kind: chrome, host: desktop, display: {host: desktop}, prepare: {kind: attach}, timeout_ms: 1000}\nrecording: {host: desktop, start: before_prepare}\nflows:\n  - path: flow.yaml\n    bindings: [{target: browser}]\n").unwrap();
        let project = Project::load(&root).unwrap();
        (root, project)
    }

    #[tokio::test]
    async fn step_sink_records_elapsed_case_step_and_scoped_or_default_target() {
        let (root, project) = fixture();
        let recorder = Recorder::new(&root, &project);
        let sink = {
            let mut recorder = recorder.lock().await;
            recorder.origin = Some(Instant::now() - Duration::from_millis(10));
            recorder.step_sink(2, "browser".into()).unwrap()
        };

        for (step_id, resource, platform, phase) in [
            (
                "native-popup",
                Some("desktop-ui"),
                None,
                robot_flow::FlowStepPhase::Started,
            ),
            (
                "native-popup",
                Some("desktop-ui"),
                None,
                robot_flow::FlowStepPhase::Passed,
            ),
            (
                "browser-return",
                None,
                None,
                robot_flow::FlowStepPhase::Started,
            ),
        ] {
            sink(robot_flow::FlowStepEvent {
                phase,
                step_id: step_id.into(),
                action: "tap".into(),
                resource: resource.map(str::to_owned),
                platform: platform.map(str::to_owned),
            });
        }
        sink(robot_flow::FlowStepEvent {
            phase: robot_flow::FlowStepPhase::Started,
            step_id: "platform-target".into(),
            action: "wait".into(),
            resource: None,
            platform: Some("windows".into()),
        });

        let recorder = recorder.lock().await;
        let events = recorder.events.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0]["event"], "step");
        assert_eq!(events[0]["phase"], "started");
        assert_eq!(events[0]["case"], 2);
        assert_eq!(events[0]["step"], "native-popup");
        assert_eq!(events[0]["action"], "tap");
        assert_eq!(events[0]["target"], "desktop-ui");
        assert_eq!(events[1]["phase"], "passed");
        assert_eq!(events[0]["resource"], "desktop-ui");
        assert_eq!(events[2]["target"], "browser");
        assert_eq!(events[3]["target"], "windows");
        assert_eq!(events[3]["platform"], "windows");
        assert!(events.iter().all(|event| {
            event["offset_ms"].as_u64().is_some()
                && !event.as_object().unwrap().contains_key("arguments")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn recorder_finalizes_once_and_validates_real_transferred_video() {
        let (root, project) = fixture();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let factory = FakeFactory {
            calls: Arc::clone(&calls),
            bytes: VIDEO.to_vec(),
        };
        let recorder = Recorder::new(&root, &project);
        prepare(&recorder, &project, &factory).await.unwrap();
        start(&recorder).await.unwrap();
        recorder.lock().await.event("case_started", Some(1));
        let sink = recorder
            .lock()
            .await
            .step_sink(1, "browser".into())
            .unwrap();
        sink(robot_flow::FlowStepEvent {
            phase: robot_flow::FlowStepPhase::Started,
            step_id: "open-home".into(),
            action: "open_url".into(),
            resource: Some("browser".into()),
            platform: None,
        });
        sink(robot_flow::FlowStepEvent {
            phase: robot_flow::FlowStepPhase::Passed,
            step_id: "open-home".into(),
            action: "open_url".into(),
            resource: Some("browser".into()),
            platform: None,
        });
        finish(&recorder).await.unwrap();
        finish(&recorder).await.unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.as_str() == "mobile_start_screen_recording")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.as_str() == "mobile_stop_screen_recording")
                .count(),
            1
        );
        assert_eq!(calls.last().unwrap(), "close");
        assert_eq!(
            std::fs::read(root.join("project-video.mp4")).unwrap(),
            VIDEO
        );
        let timeline: Value =
            serde_json::from_slice(&std::fs::read(root.join("project-timeline.json")).unwrap())
                .unwrap();
        assert!(timeline["events"].as_array().unwrap().iter().any(|event| {
            event["event"] == "step"
                && event["phase"] == "passed"
                && event["case"] == 1
                && event["step"] == "open-home"
                && event["action"] == "open_url"
                && event["target"] == "browser"
                && event["offset_ms"].as_u64().is_some()
        }));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn invalid_transferred_video_is_not_published() {
        let (root, project) = fixture();
        let factory = FakeFactory {
            calls: Arc::new(StdMutex::new(Vec::new())),
            bytes: b"not a video".to_vec(),
        };
        let recorder = Recorder::new(&root, &project);
        prepare(&recorder, &project, &factory).await.unwrap();
        start(&recorder).await.unwrap();
        assert!(finish(&recorder).await.is_err());
        assert!(!root.join("project-video.mp4").exists());
        assert!(!root.join("project-video.mp4.partial").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

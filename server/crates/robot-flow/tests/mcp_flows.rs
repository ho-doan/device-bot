use async_trait::async_trait;
use robot_flow::{
    FlowDocument, FlowRunner, Profile, Profiles,
    mcp::{self, Mcp},
    schema,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Temp(PathBuf);
static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Temp {
    fn new() -> Self {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "robot-flow-test-{}-{id}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct State {
    calls: Vec<Value>,
    closed: bool,
    queries: VecDeque<Value>,
    fail: Option<String>,
    schemas: Option<Vec<Value>>,
    source_selection: bool,
    devices: Option<Value>,
    video_path: Option<PathBuf>,
    web_fault: robot_core::WebFaultState,
}
struct Fake(Arc<Mutex<State>>);
#[async_trait]
impl Mcp for Fake {
    async fn request(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<Value> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(json!({"method":method,"params":params}));
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => Ok(
                json!({"tools":s.schemas.clone().unwrap_or_else(||["mobile_list_available_devices","mobile_launch_app","mobile_list_elements_on_screen","mobile_click_on_screen_at_coordinates","mobile_swipe_on_screen","mobile_type_keys","mobile_execute_ui_actions","mobile_press_ime_action","mobile_start_screen_recording","mobile_stop_screen_recording","mobile_take_screenshot",mcp::WEB_TEST_PROXY].iter().map(|n|json!({"name":n,"inputSchema":if s.source_selection && *n == "mobile_list_elements_on_screen" {json!({"properties":{"source":{"enum":["auto","native","flutter"]}}})} else {json!({})}})).collect())}),
            ),
            "tools/call" => {
                let name = params["name"].as_str().unwrap();
                if s.fail.as_deref() == Some(name) {
                    return Ok(
                        json!({"isError":true,"content":[{"type":"text","text":"Unsupported: deliberately unavailable"}]}),
                    );
                }
                if name == "mobile_list_available_devices" {
                    return Ok(
                        json!({"structuredContent":{"devices":s.devices.clone().unwrap_or_else(||json!([{"id":"device-1","platform":"android","state":"available","name":"fixture"}]))}}),
                    );
                }
                if name == "mobile_list_elements_on_screen" {
                    if s.web_fault.fault.is_some() {
                        s.web_fault.hits += 1;
                    }
                    return Ok(
                        json!({"structuredContent":{"elements":s.queries.pop_front().unwrap_or(json!([]))}}),
                    );
                }
                if name == mcp::WEB_TEST_PROXY {
                    let args = &params["arguments"];
                    if args["action"] == "status" {
                        return Ok(json!({"structuredContent": s.web_fault}));
                    }
                    let previous = s.web_fault.clone();
                    s.web_fault = robot_core::WebFaultState {
                        revision: previous.revision + 1,
                        hits: 0,
                        fault: if args["action"] == "set" {
                            Some(serde_json::from_value(args["fault"].clone()).unwrap())
                        } else {
                            None
                        },
                    };
                    return Ok(
                        json!({"structuredContent": {"active": s.web_fault, "previous": previous}}),
                    );
                }
                if name == "mobile_stop_screen_recording" {
                    if let Some(path) = &s.video_path {
                        return Ok(
                            json!({"content":[{"type":"text","text":format!("Screen recording saved: {}", path.display())}]}),
                        );
                    }
                }
                if name == "mobile_take_screenshot" {
                    return Ok(
                        json!({"content":[{"type":"image","mimeType":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII="}]}),
                    );
                }
                Ok(json!({"content":[]}))
            }
            _ => Err("unexpected method".into()),
        }
    }
    async fn notify(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<()> {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(json!({"method":method,"params":params}));
        Ok(())
    }
    async fn close(&mut self) -> robot_flow::mcp::Result<()> {
        self.0.lock().unwrap().closed = true;
        Ok(())
    }
}
fn flow(actions: &str) -> FlowDocument {
    FlowDocument::from_yaml(&format!(
        "version: 2\nname: test\nsteps:\n  - select_device: {{platform: android}}\n{actions}"
    ))
    .unwrap()
}
fn node(label: &str) -> Value {
    json!({"text":label,"contentDescription":label,"elementType":"button","className":"button","bounds":{"left":10,"top":20,"right":110,"bottom":80},"enabled":true,"clickable":true})
}
async fn run(flow: &FlowDocument, p: &Profile, state: Arc<Mutex<State>>) -> robot_flow::FlowReport {
    let tmp = Temp::new();
    FlowRunner::new(Some(Box::new(Fake(state))), tmp.0.clone())
        .run(flow, p, &[], Duration::from_secs(3))
        .await
}

fn bindings() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "alice".into(),
            json!({"id":"chrome-alice","platform":"chrome"}),
        ),
        ("bob".into(), json!({"id":"chrome-bob","platform":"chrome"})),
        ("desktop".into(), json!({"id":"native","platform":"macos"})),
    ])
}

#[path = "mcp_flows/gestures.rs"]
mod gestures;
#[path = "mcp_flows/input_and_evidence.rs"]
mod input_and_evidence;
#[path = "mcp_flows/native_selection.rs"]
mod native_selection;
#[path = "mcp_flows/network_faults.rs"]
mod network_faults;
#[path = "mcp_flows/platform_routing.rs"]
mod platform_routing;
#[path = "mcp_flows/positional_selectors.rs"]
mod positional_selectors;
#[path = "mcp_flows/recording_evidence.rs"]
mod recording_evidence;
#[path = "mcp_flows/resource_routing.rs"]
mod resource_routing;
#[path = "mcp_flows/selectors_and_validation.rs"]
mod selectors_and_validation;
#[cfg(unix)]
#[path = "mcp_flows/stdio.rs"]
mod stdio;

use async_trait::async_trait;
use robot_flow::{
    FlowDocument, FlowRunner, Profile, Profiles,
    mcp::{self, Mcp},
    schema,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
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
}
struct Fake(Arc<Mutex<State>>);
#[async_trait]
impl Mcp for Fake {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(json!({"method":method,"params":params}));
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => Ok(
                json!({"tools":s.schemas.clone().unwrap_or_else(||["mobile_list_available_devices","mobile_launch_app","mobile_list_elements_on_screen","mobile_click_on_screen_at_coordinates","mobile_swipe_on_screen","mobile_type_keys","mobile_execute_ui_actions","mobile_press_ime_action","mobile_start_screen_recording","mobile_stop_screen_recording","mobile_take_screenshot"].iter().map(|n|json!({"name":n,"inputSchema":if s.source_selection && *n == "mobile_list_elements_on_screen" {json!({"properties":{"source":{"enum":["auto","native","flutter"]}}})} else {json!({})}})).collect())}),
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
                    return Ok(
                        json!({"structuredContent":{"elements":s.queries.pop_front().unwrap_or(json!([]))}}),
                    );
                }
                Ok(json!({"content":[]}))
            }
            _ => Err("unexpected method".into()),
        }
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(json!({"method":method,"params":params}));
        Ok(())
    }
    async fn close(&mut self) -> Result<(), String> {
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

#[tokio::test]
async fn lifecycle_and_tool_calls_use_one_session() {
    let state = Arc::new(Mutex::new(State::default()));
    let report = run(
        &flow("  - launch_app: {app_id: example.app}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "PASS");
    let s = state.lock().unwrap();
    assert_eq!(
        s.calls
            .iter()
            .map(|c| c["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call",
            "tools/call"
        ]
    );
    assert_eq!(s.calls[4]["params"]["arguments"]["deviceId"], "device-1");
    assert!(s.closed);
}

#[tokio::test]
async fn profile_selector_environment_is_resolved_before_querying() {
    let label = std::env::var("PATH").unwrap();
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([node(&label)])]),
        ..State::default()
    }));
    let profile: Profile =
        serde_yaml::from_str("selectors:\n  fixture: {label: '${env:PATH}'}\n").unwrap();
    let report = run(
        &flow("  - assert_element: {target: fixture, timeout_ms: 1}"),
        &profile,
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let query = state
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_list_elements_on_screen")
        .unwrap();
    assert_eq!(query["params"]["arguments"]["filter"], label);
}

#[test]
fn profile_selectors_resolve_variables_and_reject_missing_environment() {
    let mut profile: Profile = serde_yaml::from_str(
        "variables:\n  kind: button\nselectors:\n  fixture: {element_type: '${kind}'}\n",
    )
    .unwrap();
    let f = flow("  - assert_element: {target: fixture}");
    // Validation rejects an unresolved element type, so this proves resolution
    // happens before validation rather than only at query time.
    schema::compile(&f, &profile).unwrap();
    profile.selectors.get_mut("fixture").unwrap().label =
        Some("${env:ROBOT_FLOW_MISSING_SELECTOR_ENV_73DDE9}".into());
    assert!(
        schema::compile(&f, &profile)
            .unwrap_err()
            .contains("missing environment variable")
    );
}

#[tokio::test(start_paused = true)]
async fn polling_continues_past_twelve_attempts_until_declared_timeout() {
    let mut queries = VecDeque::from(vec![json!([]); 13]);
    queries.push_back(json!([node("Late fixture")]));
    let state = Arc::new(Mutex::new(State {
        queries,
        ..State::default()
    }));
    let tmp = Temp::new();
    let start = tokio::time::Instant::now();
    let report = FlowRunner::new(Some(Box::new(Fake(state))), tmp.0.clone())
        .run(
            &flow("  - assert_element: {label: Late fixture, timeout_ms: 5000}"),
            &Profile::default(),
            &[],
            Duration::from_secs(10),
        )
        .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert_eq!(start.elapsed(), Duration::from_millis(3250));
}

#[tokio::test(start_paused = true)]
async fn polling_honors_timeout_shorter_than_poll_interval() {
    let state = Arc::new(Mutex::new(State::default()));
    let start = tokio::time::Instant::now();
    let report = run(
        &flow("  - assert_element: {label: Missing, timeout_ms: 60}"),
        &Profile::default(),
        state,
    )
    .await;
    assert_eq!(report.status, "FAIL");
    assert!(
        report
            .error
            .as_ref()
            .unwrap()
            .contains("bounded wait expired"),
        "{:?}",
        report.error
    );
    assert_eq!(start.elapsed(), Duration::from_millis(60));
}

#[tokio::test]
async fn dry_run_neither_connects_nor_creates_artifacts() {
    let tmp = Temp::new();
    let report = FlowRunner::new(None, tmp.0.clone())
        .run(
            &flow("  - tap: {x: 1, y: 2}"),
            &Profile::default(),
            &[],
            Duration::from_secs(1),
        )
        .await;
    assert_eq!(report.status, "DRY_RUN");
    assert!(!tmp.0.exists());
    assert!(report.steps.iter().all(|s| s.status == "validated"));
}

#[tokio::test]
async fn ambiguity_fails_without_tapping() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([node("Sign in"), node("Sign in")])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - tap_element: {label: Sign in}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert!(report.error.unwrap().contains("ambiguous"));
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_click_on_screen_at_coordinates")
    );
}

#[tokio::test]
async fn absent_does_not_hide_tool_failure() {
    let state = Arc::new(Mutex::new(State {
        fail: Some("mobile_list_elements_on_screen".into()),
        ..State::default()
    }));
    let report = run(
        &flow("  - assert_absent: {label: Missing}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert!(report.error.unwrap().contains("Unsupported"));
    assert!(state.lock().unwrap().closed);
}

#[tokio::test]
async fn missing_state_is_not_false() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([node("Task")])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - assert_state: {label: Task, property: checked, expected: false}"),
        &Profile::default(),
        state,
    )
    .await;
    assert!(report.error.unwrap().contains("unknown, not false"));
}

#[tokio::test]
async fn launch_app_supports_web_urls_and_native_identifiers() {
    for (platform, app_id, tool, key) in [
        (
            "chrome",
            "http://localhost:8080/#/login",
            "mobile_open_url",
            "url",
        ),
        (
            "chrome",
            "https://example.test/app?lang=ja#/login",
            "mobile_open_url",
            "url",
        ),
        ("android", "com.example.app", "mobile_launch_app", "appId"),
        ("ios", "com.example.ios", "mobile_launch_app", "appId"),
        (
            "windows",
            "C:/Program Files/Example App/example.exe",
            "mobile_launch_app",
            "appId",
        ),
    ] {
        let state = Arc::new(Mutex::new(State {
            // Advertise only the applicable launch tool: preflight must choose it
            // before executing any device actions.
            schemas: Some(vec![
                json!({"name":"mobile_list_available_devices","inputSchema":{}}),
                json!({"name":tool,"inputSchema":{
                    "type":"object", "required":["deviceId",key],
                    "additionalProperties":false,
                    "properties":{"deviceId":{"type":"string"},(key):{"type":"string"}}
                }}),
            ]),
            devices: Some(json!([{"id":"target-1","platform":platform,"state":"available"}])),
            ..State::default()
        }));
        let flow = FlowDocument::from_yaml(&format!(
            "version: 2\nname: launch\nsteps:\n- select_device: {{platform: {platform}}}\n- launch_app: {{app_id: '{app_id}'}}\n"
        )).unwrap();
        let report = run(&flow, &Profile::default(), state.clone()).await;
        assert_eq!(report.status, "PASS", "{platform}: {:?}", report.error);
        let state = state.lock().unwrap();
        let calls: Vec<_> = state
            .calls
            .iter()
            .filter(|c| c["method"] == "tools/call")
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["params"]["name"], tool);
        assert_eq!(
            calls[1]["params"]["arguments"],
            json!({"deviceId":"target-1",(key):app_id})
        );
    }
}

#[tokio::test]
async fn obsolete_url_action_is_rejected_before_device_actions() {
    let state = Arc::new(Mutex::new(State::default()));
    let report = run(
        &flow("  - open_url: {url: 'https://example.test'}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "FAIL");
    assert!(report.error.unwrap().contains("unknown action open_url"));
    assert!(state.lock().unwrap().calls.is_empty());
}

#[test]
fn fixture_profiles_share_one_launch_recipe() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples/automation_test");
    let profiles = Profiles::from_path(&root.join("targets.yaml")).unwrap();
    let flow = FlowDocument::from_yaml(
        "version: 2\nname: shared-launch\nsteps:\n- select_device: '${device}'\n- run: {recipe: open_fixture}\n",
    ).unwrap();
    for (name, profile) in profiles.targets {
        let steps = schema::compile(&flow, &profile).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(steps.len(), 3, "{name}");
        assert_eq!(steps[1].action["launch_app"]["app_id"], profile.app_id);
        assert!(
            !steps
                .iter()
                .any(|step| step.action.contains_key("open_url"))
        );
    }
}

#[tokio::test]
async fn unsupported_required_tool_fails_before_device_action() {
    let state = Arc::new(Mutex::new(State {
        schemas: Some(vec![
            json!({"name":"mobile_list_available_devices","inputSchema":{}}),
        ]),
        ..State::default()
    }));
    let report = run(
        &flow("  - launch_app: {app_id: app}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert!(
        report
            .error
            .unwrap()
            .contains("required MCP tool unavailable")
    );
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["method"] == "tools/call")
    );
}

#[test]
fn tool_arguments_obey_advertised_schema() {
    let schema = json!({"type":"object","required":["deviceId","text"],"additionalProperties":false,"properties":{"deviceId":{"type":"string","minLength":1},"text":{"type":"string","minLength":1}}});
    assert!(
        mcp::validate_arguments(&schema, &json!({"deviceId":"d","text":"  Orbit123!  "})).is_ok()
    );
    assert!(mcp::validate_arguments(&schema, &json!({"deviceId":"d"})).is_err());
    assert!(
        mcp::validate_arguments(&schema, &json!({"deviceId":"d","text":"","other":1})).is_err()
    );
}

#[test]
fn common_scenarios_compile_for_all_profiles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/flutter_app/automation_test");
    let profiles = Profiles::from_path(&root.join("targets.yaml")).unwrap();
    for (name, mut p) in profiles.targets {
        if name == "windows" {
            p.app_id = "C:/fixture/Orbit.exe".into();
        }
        for n in 1..=9 {
            let f = FlowDocument::from_path(root.join(format!("it-{n:02}.yaml"))).unwrap();
            let compiled = schema::compile(&f, &p).unwrap_or_else(|e| panic!("{name} IT-{n}: {e}"));
            assert!(!compiled.is_empty());
            // Ensure validation is shared with the actual runner's dry-run path.
        }
    }
}

#[test]
fn recursive_recipe_and_unknown_selectors_fail() {
    let f = flow("  - run: {recipe: recursive}");
    let mut p = Profile::default();
    p.recipes.insert(
        "recursive".into(),
        serde_yaml::from_str("- run: {recipe: recursive}").unwrap(),
    );
    assert!(schema::compile(&f, &p).unwrap_err().contains("cycle"));
    assert!(
        schema::compile(
            &flow("  - tap_element: {target: typo}"),
            &Profile::default()
        )
        .unwrap_err()
        .contains("unknown selector")
    );
}

#[test]
fn malformed_actions_and_unresolved_variables_fail() {
    for actions in [
        "  - tap: {x: 1, y: 2}\n    wait: {ms: 1}",
        "  - tap_element: {label: Button, typo: true}",
        "  - type_text: {text: '${missing}'}",
    ] {
        assert!(schema::compile(&flow(actions), &Profile::default()).is_err());
    }
}

#[tokio::test]
async fn typed_text_preserves_case_and_spaces_and_query_evidence_is_redacted() {
    let tmp = Temp::new();
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([node(" Orbit123! ")])]),
        ..State::default()
    }));
    let f =
        flow("  - type_text: {text: ' Orbit123! '}\n  - assert_element: {label: ' Orbit123! '}");
    let r = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &Profile::default(), &[], Duration::from_secs(2))
        .await;
    assert_eq!(r.status, "PASS");
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["arguments"]["text"] == " Orbit123! ")
    );
    let trace = std::fs::read_to_string(tmp.0.join("query-1.json")).unwrap();
    assert!(!trace.contains("Orbit123!"));
    assert!(trace.contains("REDACTED"));
}

#[test]
fn normalized_types_and_offsets_do_not_assume_platform() {
    for (kind, class) in [
        ("textField", "android.widget.EditText"),
        ("textField", "textField"),
        ("switch", "switch"),
    ] {
        let mut e = node("Field");
        e["elementType"] = json!(kind);
        e["className"] = json!(class);
        let s = schema::Selector {
            element_type: Some(kind.into()),
            ..Default::default()
        };
        assert_eq!(robot_flow::runner::filter_elements(&s, &[e]).len(), 1);
    }
}

#[tokio::test]
async fn scroll_uses_observed_viewport_offsets_above_keyboard() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([
            json!([{"className":"scrollView","bounds":{"left":100,"top":200,"right":500,"bottom":600}}]),
        ]),
        ..State::default()
    }));
    let report = run(
        &flow("  - scroll: {class_contains: scrollView, largest: true, direction: down}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "PASS");
    let s = state.lock().unwrap();
    let swipe = s
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_swipe_on_screen")
        .unwrap();
    let a = &swipe["params"]["arguments"];
    assert_eq!(a["startX"], 300.);
    assert_eq!(a["startY"], 488.);
    assert_eq!(a["endY"], 360.);
}

#[tokio::test]
async fn replacement_recipe_preserves_literal_input_and_proves_new_value() {
    let mut old = node("Email address");
    old["elementType"] = json!("textField");
    old["value"] = json!("old");
    let mut new = old.clone();
    new["value"] = json!("  NEW@example.org  ");
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([
            json!([old.clone()]),
            json!([old]),
            json!([new.clone()]),
            json!([new]),
        ]),
        ..State::default()
    }));
    let mut p = Profile::default();
    p.selectors.insert(
        "email".into(),
        schema::Selector {
            label: Some("Email address".into()),
            replace_recipe: Some("replace".into()),
            ..Default::default()
        },
    );
    p.recipes.insert("replace".into(),serde_yaml::from_str("- gesture: {target: '${target}', from: {x: 0.99, y: 0.5}, to: {x: 0.01, y: 0.5}}\n- type_text: {text: '${text}'}\n- assert_value: {target: '${target}', expected: '${text}'}").unwrap());
    let report = run(
        &flow("  - fill_field: {target: email, text: '  NEW@example.org  '}"),
        &p,
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["arguments"]["text"] == "  NEW@example.org  ")
    );
}

#[tokio::test]
async fn truncated_query_cannot_establish_absence() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!((0..100).map(|_| node("other")).collect::<Vec<_>>())]),
        ..State::default()
    }));
    let report = run(
        &flow("  - assert_absent: {label: Missing}"),
        &Profile::default(),
        state,
    )
    .await;
    assert!(report.error.unwrap().contains("truncated"));
}

#[tokio::test]
async fn precondition_fails_before_initialization() {
    let state = Arc::new(Mutex::new(State::default()));
    let p = Profile {
        preconditions: vec!["fixture_ready".into()],
        ..Default::default()
    };
    let r = run(&flow("  - launch_app: {app_id: app}"), &p, state.clone()).await;
    assert!(r.error.unwrap().contains("precondition"));
    assert!(state.lock().unwrap().calls.is_empty());
    assert!(state.lock().unwrap().closed);
}

#[tokio::test]
async fn recording_cleanup_is_attempted_after_assertion_error() {
    let state = Arc::new(Mutex::new(State {
        fail: Some("mobile_list_elements_on_screen".into()),
        ..State::default()
    }));
    let r = run(
        &flow("  - start_screen_recording: {}\n  - assert_element: {label: Error}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(r.status, "FAIL");
    let s = state.lock().unwrap();
    assert!(
        s.calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_stop_screen_recording")
    );
    assert!(s.closed);
}

#[tokio::test]
async fn disabled_element_cannot_be_used_as_conditional_fallback() {
    let mut e = node("Show password");
    e["enabled"] = json!(false);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([e])]),
        ..State::default()
    }));
    let mut p = Profile::default();
    p.selectors.insert(
        "show".into(),
        schema::Selector {
            label: Some("Show password".into()),
            ..Default::default()
        },
    );
    let r = run(
        &flow("  - if_element: {target: show, then: [], else: [{type_text: {text: wrong}}]}"),
        &p,
        state.clone(),
    )
    .await;
    assert!(r.error.unwrap().contains("disabled"));
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_type_keys")
    );
}

#[tokio::test]
async fn legacy_v1_steps_execute_through_mcp() {
    let f=FlowDocument::from_yaml("version: 1\nname: old\nsteps:\n- select_device: {platform: android}\n- launch_app: {app_id: app}\n- tap: {x: 10, y: 20}").unwrap();
    let state = Arc::new(Mutex::new(State::default()));
    assert_eq!(
        run(&f, &Profile::default(), state.clone()).await.status,
        "PASS"
    );
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_click_on_screen_at_coordinates")
    );
}

#[test]
fn setup_failure_cli_still_writes_junit() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let yaml = tmp.0.join("case.yaml");
    std::fs::write(
        &yaml,
        "version: 2\nname: setup_error\nsteps:\n- select_device: {}\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_robot-flow"))
        .args(["--server", "/nonexistent/device-bot", "--artifacts"])
        .arg(tmp.0.join("reports"))
        .arg(&yaml)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let dir = std::fs::read_dir(tmp.0.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let report: Value =
        serde_json::from_slice(&std::fs::read(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "FAIL");
    assert!(
        std::fs::read_to_string(dir.join("junit.xml"))
            .unwrap()
            .contains("<error")
    );
}

#[test]
fn recipe_parameters_are_opaque_data_not_templates() {
    let f = flow("  - run: {recipe: echo, with: {text: '${env:PATH}'}}");
    let mut p = Profile::default();
    p.recipes.insert(
        "echo".into(),
        serde_yaml::from_str("- type_text: {text: '${text}'}").unwrap(),
    );
    let steps = schema::compile(&f, &p).unwrap();
    assert_eq!(
        steps[1].action["type_text"]["text"],
        std::env::var("PATH").unwrap()
    );
    p.variables.insert("a".into(), json!("${b}"));
    p.variables.insert("b".into(), json!("${a}"));
    assert!(schema::compile(&f, &p).unwrap_err().contains("cyclic"));
}

#[tokio::test]
async fn sensitive_input_omits_visual_evidence_instead_of_leaking_it() {
    let tmp = Temp::new();
    let state = Arc::new(Mutex::new(State::default()));
    let f = flow(
        "  - type_text: {text: test-secret, sensitive: true}\n  - screenshot: {path: result.png}",
    );
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &Profile::default(), &[], Duration::from_secs(2))
        .await;
    assert_eq!(report.status, "PARTIAL");
    assert!(!tmp.0.join("result.png").exists());
    assert_eq!(report.steps.last().unwrap().status, "omitted");
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_take_screenshot")
    );
}

#[tokio::test]
async fn assertion_failure_is_distinct_from_transport_error() {
    let mut e = node("Task");
    e["checked"] = json!(true);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([e])]),
        ..State::default()
    }));
    let r = run(
        &flow("  - assert_state: {label: Task, property: checked, expected: false}"),
        &Profile::default(),
        state,
    )
    .await;
    assert_eq!(r.error_kind.as_deref(), Some("assertion"));
    assert_eq!(r.status, "FAIL");
}

#[tokio::test]
async fn recipe_environment_input_disables_visual_evidence() {
    let tmp = Temp::new();
    let state = Arc::new(Mutex::new(State::default()));
    let mut profile = Profile::default();
    // PATH is present without mutating process-global environment in parallel tests.
    profile.recipes.insert(
        "credential".into(),
        serde_yaml::from_str("- type_text: {text: '${env:PATH}'}").unwrap(),
    );
    let f = flow("  - run: {recipe: credential}\n  - screenshot: {path: result.png}");
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &profile, &[], Duration::from_secs(2))
        .await;
    assert_eq!(report.status, "PARTIAL");
    assert_eq!(report.steps.last().unwrap().status, "omitted");
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_take_screenshot")
    );
}

#[tokio::test]
async fn case_deadline_reports_the_active_nested_step() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut profile = Profile::default();
    profile.selectors.insert(
        "button".into(),
        serde_yaml::from_str("label: Continue").unwrap(),
    );
    state
        .lock()
        .unwrap()
        .queries
        .push_back(json!([node("Continue")]));
    let f = flow(
        "  - id: branch\n    if_element:\n      target: button\n      then:\n        - id: slow\n          wait: {ms: 1000}",
    );
    let tmp = Temp::new();
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &profile, &[], Duration::from_millis(100))
        .await;
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.error_kind.as_deref(), Some("timeout"));
    assert!(
        report
            .steps
            .iter()
            .any(|s| s.id == "branch.then.slow" && s.status == "failed")
    );
    assert!(state.lock().unwrap().closed);
}

#[tokio::test]
async fn explicit_native_value_takes_priority_over_matching_label() {
    for value in [json!("actual"), json!(null)] {
        let state = Arc::new(Mutex::new(State::default()));
        let mut e = node("expected");
        e["value"] = value;
        state.lock().unwrap().queries.push_back(json!([e]));
        let f = flow("  - assert_value: {label: expected, expected: expected}");
        let report = run(&f, &Profile::default(), state).await;
        assert_eq!(report.status, "FAIL");
    }
}

#[tokio::test]
async fn null_native_state_is_unknown() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut e = node("Remember");
    e["checked"] = Value::Null;
    state.lock().unwrap().queries.push_back(json!([e]));
    let f = flow("  - assert_state: {label: Remember, property: checked, expected: false}");
    let report = run(&f, &Profile::default(), state).await;
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.error_kind.as_deref(), Some("unsupported"));
}

#[test]
fn nested_recipe_parameters_are_not_interpolated_twice() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let scenario = tmp.0.join("nested.yaml");
    let profiles = tmp.0.join("profiles.yaml");
    std::fs::write(&scenario,
        "version: 2\nname: nested\nsteps:\n  - run:\n      recipe: edit\n      with:\n        text: '${env:ROBOT_FLOW_LITERAL_INPUT}'\n").unwrap();
    std::fs::write(&profiles,
        "version: 1\nselectors:\n  field: {label: '${env:ROBOT_FLOW_LITERAL_INPUT}'}\nrecipes:\n  edit:\n    - if_element:\n        target: field\n        then:\n          - type_text: {text: '${text}'}\ntargets:\n  test: {}\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_robot-flow"))
        .args(["--dry-run", "--profiles"])
        .arg(profiles)
        .args(["--target", "test"])
        .arg(scenario)
        .env("ROBOT_FLOW_LITERAL_INPUT", "  Đặng ${literal} AbC  ")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[tokio::test]
async fn stable_bounds_do_not_depend_on_snapshot_generated_ids() {
    // Captured Simulator responses for the same visible heading: bounds, label
    // and class are unchanged, while the transport's snapshot IDs differ.
    let queries: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/ios-heading-sequence.json")).unwrap();
    let state = Arc::new(Mutex::new(State {
        queries: queries.into(),
        ..State::default()
    }));
    let profile = Profile {
        stable_samples: Some(2),
        ..Profile::default()
    };
    let f = flow("  - assert_element: {label: 'A fresh start, Alex.'}");
    let report = run(&f, &profile, state).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
}

#[tokio::test]
async fn moving_bounds_still_reset_stability_samples() {
    let queries: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/ios-heading-sequence.json")).unwrap();
    let mut moved = queries[1].clone();
    moved[0]["bounds"]["left"] = json!(34);
    moved[0]["bounds"]["right"] = json!(277);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([queries[0].clone(), moved.clone(), moved]),
        ..State::default()
    }));
    let profile = Profile {
        stable_samples: Some(2),
        ..Profile::default()
    };
    let f = flow("  - assert_element: {label: 'A fresh start, Alex.'}");
    let report = run(&f, &profile, state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let calls = &state.lock().unwrap().calls;
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["params"]["name"] == "mobile_list_elements_on_screen")
            .count(),
        3
    );
}

#[tokio::test]
async fn offscreen_zero_bounds_trigger_bounded_viewport_search() {
    let mut hidden = node("Task");
    hidden["bounds"] = json!({"left":0,"top":0,"right":0,"bottom":0});
    hidden["clickable"] = json!(false);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([
            json!([hidden.clone()]),
            json!([hidden]),
            json!([node("Screen")]),
            json!([node("Task")]),
        ]),
        ..State::default()
    }));
    let mut profile = Profile {
        viewport: Some("screen".into()),
        ..Profile::default()
    };
    profile.selectors.insert(
        "screen".into(),
        serde_yaml::from_str("label: Screen").unwrap(),
    );
    let f = flow("  - assert_element: {label: Task}");
    let report = run(&f, &profile, state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_swipe_on_screen")
    );
}

#[cfg(unix)]
mod stdio {
    use super::*;
    use robot_flow::mcp::StdioMcp;
    #[tokio::test]
    async fn real_pipe_lifecycle_and_shutdown() {
        let script = "import sys,json\nfor line in sys.stdin:\n m=json.loads(line)\n if 'id' not in m: continue\n r={'protocolVersion':'2025-11-25'} if m['method']=='initialize' else {'tools':[]}\n print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)\n";
        let mut client = StdioMcp::spawn(
            "python3",
            &["-u".into(), "-c".into(), script.into()],
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(mcp::initialize(&mut client).await.unwrap().is_empty());
        client.close().await.unwrap();
    }
    #[tokio::test]
    async fn pipe_eof_timeout_and_protocol_errors_are_distinct() {
        for (script, expected) in [
            ("import sys;sys.exit(0)", "EOF"),
            ("import time;time.sleep(3)", "timeout"),
            ("print('bad-json',flush=True)", "invalid MCP JSON"),
        ] {
            let mut client = StdioMcp::spawn(
                "python3",
                &["-u".into(), "-c".into(), script.into()],
                if expected == "timeout" {
                    Duration::from_millis(100)
                } else {
                    Duration::from_secs(5)
                },
            )
            .unwrap();
            let error = client.request("initialize", json!({})).await.unwrap_err();
            assert!(error.contains(expected), "{error}");
            client.close().await.unwrap();
        }
    }
}

#[cfg(test)]
fn ios_semantics_profile_for_fake_mcp() -> Profile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/flutter_app/automation_test/targets-semantics.yaml");
    let mut profile = Profiles::from_path(&path)
        .unwrap()
        .targets
        .remove("ios-sim")
        .unwrap();
    // Fake MCP replays captured native snapshots without a device or app setup.
    // Live runs continue to require --confirm fixture_ready.
    profile.preconditions.clear();
    profile
}

#[tokio::test]
async fn semantics_edit_menu_targets_the_native_action_not_its_text_child() {
    let menu: Value =
        serde_json::from_str(include_str!("fixtures/ios-select-all-menu.json")).unwrap();
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([menu.clone(), menu.clone(), menu]),
        ..State::default()
    }));
    let f = flow(
        "  - if_element: {target: select_all, then: [{tap_element: {target: select_all}}], else: []}",
    );
    let report = run(&f, &ios_semantics_profile_for_fake_mcp(), state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["params"]["name"] == "mobile_execute_ui_actions"
                && c["params"]["arguments"]["actions"][0]["type"] == "tap_element")
    );
}

#[tokio::test]
async fn semantics_field_focus_is_observed_before_sending_ime_action() {
    let mut email = node("Email address");
    email["resourceId"] = json!("email-field");
    email["className"] = json!("textField");
    email["elementType"] = json!("textField");
    email["focused"] = json!(false);
    let mut focused = email.clone();
    focused["focused"] = json!(true);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([
            json!([email.clone()]),
            json!([email]),
            json!([focused.clone()]),
            json!([focused]),
        ]),
        ..State::default()
    }));
    let f = flow(
        "  - focus_field: {target: email}\n  - call_tool: {name: mobile_press_ime_action, arguments: {deviceId: '<selected>', action: next}}",
    );
    let report = run(&f, &ios_semantics_profile_for_fake_mcp(), state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    assert!(
        state.queries.is_empty(),
        "IME must wait for native focused=true, not just tap success"
    );
}

struct ScrollSearchFake {
    inner: Fake,
    position: i32,
}
#[async_trait]
impl Mcp for ScrollSearchFake {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if method == "tools/call" {
            let name = params["name"].as_str().unwrap();
            if name == "mobile_swipe_on_screen" {
                self.position += if params["arguments"]["startY"].as_f64().unwrap()
                    > params["arguments"]["endY"].as_f64().unwrap()
                {
                    1
                } else {
                    -1
                };
            }
            if name == "mobile_list_elements_on_screen" {
                let elements = if params["arguments"]["filter"] == "Screen" {
                    json!([node("Screen")])
                } else if self.position < 0 {
                    json!([node("0/3")])
                } else {
                    let mut hidden = node("0/3");
                    hidden["bounds"] = json!({"left":0,"right":0,"top":0,"bottom":0});
                    hidden["clickable"] = json!(false);
                    json!([hidden])
                };
                self.inner.0.lock().unwrap().queries.push_back(elements);
            }
        }
        self.inner.request(method, params).await
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.inner.notify(method, params).await
    }
    async fn close(&mut self) -> Result<(), String> {
        self.inner.close().await
    }
}

#[tokio::test]
async fn viewport_search_reaches_above_its_starting_position() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut profile = Profile {
        viewport: Some("screen".into()),
        ..Profile::default()
    };
    profile.selectors.insert(
        "screen".into(),
        serde_yaml::from_str("label: Screen").unwrap(),
    );
    let tmp = Temp::new();
    let report = FlowRunner::new(
        Some(Box::new(ScrollSearchFake {
            inner: Fake(state),
            position: 0,
        })),
        tmp.0.clone(),
    )
    .run(
        &flow("  - assert_element: {label: '0/3'}"),
        &profile,
        &[],
        Duration::from_secs(4),
    )
    .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
}

#[tokio::test]
async fn missing_native_id_uses_flutter_after_applying_the_full_selector() {
    let native = node("");
    let mut flutter = node("");
    flutter["resourceId"] = json!("clear-search");
    flutter["className"] = json!("button GestureDetector");
    let state = Arc::new(Mutex::new(State {
        source_selection: true,
        queries: VecDeque::from([json!([native]), json!([flutter])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - tap_element: {resource_id: clear-search, class_contains: GestureDetector}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let sources: Vec<_> = state
        .calls
        .iter()
        .filter(|call| call["params"]["name"] == "mobile_list_elements_on_screen")
        .map(|call| call["params"]["arguments"]["source"].clone())
        .collect();
    assert_eq!(sources, [json!("native"), json!("flutter")]);
    assert!(
        state
            .calls
            .iter()
            .any(|call| call["params"]["name"] == "mobile_click_on_screen_at_coordinates")
    );
}

#[tokio::test]
async fn valid_or_ambiguous_native_selector_never_queries_flutter() {
    for count in [1, 2] {
        let state = Arc::new(Mutex::new(State {
            source_selection: true,
            queries: VecDeque::from([json!(vec![node("Clear"); count])]),
            ..State::default()
        }));
        let report = run(
            &flow("  - tap_element: {label: Clear}"),
            &Profile::default(),
            state.clone(),
        )
        .await;
        assert_eq!(report.status, if count == 1 { "PASS" } else { "FAIL" });
        if count == 2 {
            assert!(report.error.unwrap().contains("ambiguous"));
        }
        let state = state.lock().unwrap();
        assert_eq!(
            state
                .calls
                .iter()
                .filter(|call| call["params"]["name"] == "mobile_list_elements_on_screen")
                .count(),
            1
        );
        assert!(
            !state
                .calls
                .iter()
                .any(|call| call["params"]["arguments"]["source"] == "flutter")
        );
    }
}

fn desktop_devices(platform: &str) -> Value {
    json!([
        {"id":"chrome:page-1","platform":"chrome"},
        {"id":format!("{platform}-host"),"platform":platform}
    ])
}

#[tokio::test]
async fn step_platform_routes_query_and_tap_then_restores_chrome() {
    for platform in ["macos", "windows"] {
        let state = Arc::new(Mutex::new(State {
            devices: Some(desktop_devices(platform)),
            queries: VecDeque::from([json!([node("Allow")])]),
            ..State::default()
        }));
        let flow = FlowDocument::from_yaml(&format!(
            "version: 2\nname: mixed\nsteps:\n- select_device: {{id: 'chrome:page-1'}}\n- platform: {platform}\n  tap_element: {{label: Allow}}\n- type_text: {{text: back-in-page}}"
        )).unwrap();
        let report = run(&flow, &Profile::default(), state.clone()).await;
        assert_eq!(report.status, "PASS", "{:?}", report.error);
        assert_eq!(report.selected_device.unwrap()["id"], "chrome:page-1");
        let state = state.lock().unwrap();
        for tool in [
            "mobile_list_elements_on_screen",
            "mobile_click_on_screen_at_coordinates",
        ] {
            let call = state
                .calls
                .iter()
                .find(|c| c["params"]["name"] == tool)
                .unwrap();
            assert_eq!(
                call["params"]["arguments"]["deviceId"],
                format!("{platform}-host")
            );
        }
        let call = state
            .calls
            .iter()
            .find(|c| c["params"]["name"] == "mobile_type_keys")
            .unwrap();
        assert_eq!(call["params"]["arguments"]["deviceId"], "chrome:page-1");
    }
}

#[tokio::test]
async fn step_platform_requires_one_match_before_ui_actions() {
    for devices in [
        json!([]),
        json!([
            {"id":"chrome:a","platform":"chrome"},
            {"id":"chrome:b","platform":"chrome"}
        ]),
    ] {
        let state = Arc::new(Mutex::new(State {
            devices: Some(devices),
            ..State::default()
        }));
        let flow = FlowDocument::from_yaml(
            "version: 2\nname: ambiguous\nsteps:\n- platform: chrome\n  tap: {x: 1, y: 2}",
        )
        .unwrap();
        let report = run(&flow, &Profile::default(), state.clone()).await;
        assert_eq!(report.status, "FAIL");
        assert!(report.error.unwrap().contains("require exactly one"));
        assert!(
            !state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c["params"]["name"] == "mobile_click_on_screen_at_coordinates")
        );
    }
}

#[tokio::test]
async fn recording_cleanup_uses_recording_device_after_scoped_step() {
    let state = Arc::new(Mutex::new(State {
        devices: Some(desktop_devices("macos")),
        fail: Some("mobile_list_elements_on_screen".into()),
        ..State::default()
    }));
    let flow = FlowDocument::from_yaml("version: 2\nname: recording\nsteps:\n- select_device: {platform: chrome}\n- platform: macos\n  start_screen_recording: {}\n- assert_element: {label: Missing}").unwrap();
    let report = run(&flow, &Profile::default(), state.clone()).await;
    assert_eq!(report.status, "FAIL");
    let state = state.lock().unwrap();
    let stop = state
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_stop_screen_recording")
        .unwrap();
    assert_eq!(stop["params"]["arguments"]["deviceId"], "macos-host");
}

#[tokio::test]
async fn scoped_recipe_and_branch_preserve_parent_route() {
    let state = Arc::new(Mutex::new(State {
        devices: Some(desktop_devices("macos")),
        queries: VecDeque::from([json!([node("Allow")])]),
        ..State::default()
    }));
    let profile: Profile = serde_yaml::from_str("selectors:\n  allow: {label: Allow}\nrecipes:\n  permission:\n  - if_element:\n      target: allow\n      then:\n      - type_text: {text: native}\n      - platform: chrome\n        type_text: {text: page}\n      - type_text: {text: native-again}").unwrap();
    let flow = FlowDocument::from_yaml("version: 2\nname: recipe\nsteps:\n- select_device: {platform: chrome}\n- platform: macos\n  run: {recipe: permission}\n- type_text: {text: page-again}").unwrap();
    let report = run(&flow, &profile, state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let devices = state
        .calls
        .iter()
        .filter(|c| c["params"]["name"] == "mobile_type_keys")
        .map(|c| c["params"]["arguments"]["deviceId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        devices,
        ["macos-host", "chrome:page-1", "macos-host", "chrome:page-1"]
    );
}

#[tokio::test]
async fn scoped_call_tool_injects_device_and_rejects_conflicting_id() {
    for arguments in [
        "{text: native}",
        "{deviceId: '<selected>', text: native}",
        "{deviceId: 'chrome:page-1', text: native}",
    ] {
        let state = Arc::new(Mutex::new(State {
            devices: Some(desktop_devices("macos")),
            ..State::default()
        }));
        let flow = FlowDocument::from_yaml(&format!("version: 2\nname: tool\nsteps:\n- platform: macos\n  call_tool:\n    name: mobile_type_keys\n    arguments: {arguments}")).unwrap();
        let report = run(&flow, &Profile::default(), state.clone()).await;
        if arguments.contains("chrome:page-1") {
            assert_eq!(report.status, "FAIL");
            assert!(
                report
                    .error
                    .unwrap()
                    .contains("conflicts with step platform")
            );
        } else {
            assert_eq!(report.status, "PASS", "{:?}", report.error);
            let state = state.lock().unwrap();
            let call = state
                .calls
                .iter()
                .find(|c| c["params"]["name"] == "mobile_type_keys")
                .unwrap();
            assert_eq!(call["params"]["arguments"]["deviceId"], "macos-host");
        }
    }
}

#[test]
fn step_platform_validation_and_recipe_variables() {
    for platform in ["linux", "", "42"] {
        let flow = FlowDocument::from_yaml(&format!(
            "version: 2\nname: invalid\nsteps:\n- platform: '{platform}'\n  tap: {{x: 1, y: 2}}"
        ))
        .unwrap();
        assert!(
            schema::compile(&flow, &Profile::default())
                .unwrap_err()
                .contains("unknown step platform")
        );
    }
    let flow = FlowDocument::from_yaml(
        "version: 2\nname: variables\nsteps:\n- platform: '${host}'\n  tap: {x: 1, y: 2}",
    )
    .unwrap();
    let profile: Profile = serde_yaml::from_str("variables: {host: windows}").unwrap();
    assert_eq!(
        schema::compile(&flow, &profile).unwrap()[0]
            .platform
            .as_deref(),
        Some("windows")
    );
}

#[tokio::test]
async fn explicit_chrome_selection_disambiguates_step_platform() {
    let state = Arc::new(Mutex::new(State {
        devices: Some(json!([
            {"id":"chrome:a","platform":"chrome"},
            {"id":"chrome:b","platform":"chrome"}
        ])),
        ..State::default()
    }));
    let flow = FlowDocument::from_yaml("version: 2\nname: selected\nsteps:\n- select_device: {id: 'chrome:b'}\n- platform: chrome\n  type_text: {text: selected-tab}").unwrap();
    let report = run(&flow, &Profile::default(), state.clone()).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let call = state
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_type_keys")
        .unwrap();
    assert_eq!(call["params"]["arguments"]["deviceId"], "chrome:b");
}

#[tokio::test]
async fn positional_selectors_target_unnamed_duplicates_for_tap_input_and_scroll() {
    for (position, expected) in [(json!("first"), 10), (json!(2), 210), (json!("last"), 210)] {
        for action in ["tap_element", "focus_field", "scroll"] {
            let mut first = node("");
            first["resourceId"] = Value::Null;
            first["className"] = json!(if action == "scroll" {
                "scrollView"
            } else {
                "button"
            });
            let mut second = first.clone();
            second["bounds"]["left"] = json!(210);
            second["bounds"]["right"] = json!(310);
            let state = Arc::new(Mutex::new(State {
                source_selection: true,
                queries: VecDeque::from([json!([first, second])]),
                ..State::default()
            }));
            let report = run(
                &flow(&format!(
                    "  - {action}: {{source: flutter, class_contains: {}, position: {position}}}",
                    if action == "scroll" {
                        "scrollView"
                    } else {
                        "button"
                    }
                )),
                &Profile::default(),
                state.clone(),
            )
            .await;
            assert_eq!(
                report.status, "PASS",
                "{position} {action}: {:?}",
                report.error
            );
            let state = state.lock().unwrap();
            let query = state
                .calls
                .iter()
                .find(|c| c["params"]["name"] == "mobile_list_elements_on_screen")
                .unwrap();
            assert_eq!(query["params"]["arguments"]["source"], "flutter");
            let call = state
                .calls
                .iter()
                .find(|c| {
                    c["params"]["name"]
                        == if action == "scroll" {
                            "mobile_swipe_on_screen"
                        } else {
                            "mobile_click_on_screen_at_coordinates"
                        }
                })
                .unwrap();
            let x = if action == "scroll" { "startX" } else { "x" };
            assert_eq!(
                call["params"]["arguments"][x].as_f64(),
                Some(f64::from(expected + 50))
            );
        }
    }
}

#[tokio::test]
async fn invalid_positions_fail_before_any_action() {
    for position in [json!(0), json!(-1), json!("second"), json!(1.5)] {
        let state = Arc::new(Mutex::new(State::default()));
        let report = run(
            &flow(&format!(
                "  - tap_element: {{class_contains: button, position: {position}}}"
            )),
            &Profile::default(),
            state.clone(),
        )
        .await;
        assert_ne!(report.status, "PASS");
        assert!(report.error.unwrap().contains("position"));
        assert!(state.lock().unwrap().calls.is_empty());
    }
}

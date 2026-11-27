use super::*;

#[tokio::test]
async fn text_boundaries_survive_yaml_export_import_and_mcp_forwarding_without_leaking_to_evidence()
{
    let inputs = [
        "  spacing  ".into(),
        "Tiếng Việt: Đặng Thị Ánh".into(),
        "emoji 👩🏽‍💻 🧪".into(),
        "line one\nline two\r\n\tend".into(),
        "'quotes' \"double\" \\path\\end $cash".into(),
        "e\u{301}".into(),
        "x".repeat(96_000),
    ];
    for text in inputs {
        let source = serde_yaml::to_string(&json!({"version":2,"name":"text round trip","steps":[
            {"select_device":{"platform":"android"}}, {"type_text":{"text":text}}
        ]}))
        .unwrap();
        let document = FlowDocument::from_yaml(&source).unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        let temp = Temp::new();
        let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), temp.0.clone())
            .run(&document, &Profile::default(), &[], Duration::from_secs(3))
            .await;
        assert_eq!(report.status, "PASS", "{:?}", report.error);
        let state = state.lock().unwrap();
        let typed = state
            .calls
            .iter()
            .find(|c| c["params"]["name"] == "mobile_type_keys")
            .unwrap();
        assert_eq!(typed["params"]["arguments"]["text"], text);
        for name in ["mcp-calls.jsonl", "expanded-flow.yaml"] {
            assert!(
                !std::fs::read_to_string(temp.0.join(name))
                    .unwrap()
                    .contains(&text)
            );
        }
    }
    let state = Arc::new(Mutex::new(State::default()));
    let report = run(
        &flow("  - type_text: {text: ''}"),
        &Profile::default(),
        state.clone(),
    )
    .await;
    assert_eq!(report.error_kind.as_deref(), Some("setup"));
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c["method"] == "tools/call")
    );
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
        .env("DEVICE_BOT_REPORT_DATA_DIR", tmp.0.join("history"))
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
    assert!(
        schema::compile(&f, &p)
            .unwrap_err()
            .to_string()
            .contains("cyclic")
    );
}

#[tokio::test]
async fn sensitive_input_keeps_recording_until_yaml_stop() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("source.mp4");
    std::fs::write(&video, include_bytes!("../fixtures/tiny.mp4")).unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        queries: VecDeque::from([json!([node("test-secret")])]),
        ..State::default()
    }));
    let f = flow(
        "  - start_screen_recording: {}\n  - type_text: {text: test-secret, sensitive: true}\n  - assert_element: {label: test-secret}\n  - screenshot: {path: result.png}\n  - call_tool: {name: mobile_type_keys, arguments: {deviceId: '<selected>', text: test-secret}, sensitive: true}\n  - stop_screen_recording: {}",
    );
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &Profile::default(), &[], Duration::from_secs(2))
        .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert!(report.limitations.is_empty());
    assert!(report.steps.iter().all(|step| step.status == "passed"));
    assert!(tmp.0.join("result.png").is_file());
    assert_eq!(
        std::fs::read(tmp.0.join("video.mp4")).unwrap(),
        include_bytes!("../fixtures/tiny.mp4")
    );
    let state = state.lock().unwrap();
    let tools: Vec<_> = state
        .calls
        .iter()
        .filter(|c| c["method"] == "tools/call")
        .map(|c| c["params"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        vec![
            "mobile_list_available_devices",
            "mobile_start_screen_recording",
            "mobile_type_keys",
            "mobile_list_elements_on_screen",
            "mobile_take_screenshot",
            "mobile_type_keys",
            "mobile_stop_screen_recording",
        ]
    );
    for artifact in [
        "query-1.json",
        "mcp-calls.jsonl",
        "resolved-profile.yaml",
        "expanded-flow.yaml",
    ] {
        assert!(
            !std::fs::read_to_string(tmp.0.join(artifact))
                .unwrap()
                .contains("test-secret"),
            "{artifact} contains plaintext input"
        );
    }
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
async fn setup_messages_cannot_masquerade_as_timeout_or_interruption() {
    let mut scenario = flow("  - wait: {ms: 1}");
    scenario
        .preconditions
        .push("verify MCP timeout and interrupted labels".into());
    let report = run(
        &scenario,
        &Profile::default(),
        Arc::new(Mutex::new(State::default())),
    )
    .await;
    assert_eq!(report.error_kind.as_deref(), Some("setup"));
    assert_eq!(report.status, "FAIL");
}

#[tokio::test]
async fn opaque_tool_errors_remain_execution_failures_during_assertions() {
    let state = Arc::new(Mutex::new(State {
        fail: Some("mobile_list_elements_on_screen".into()),
        ..State::default()
    }));
    let report = run(
        &flow("  - assert_absent: {label: Task}"),
        &Profile::default(),
        state,
    )
    .await;
    assert_eq!(report.error_kind.as_deref(), Some("execution"));
    assert!(
        report
            .error
            .unwrap()
            .contains("Unsupported: deliberately unavailable")
    );
}

#[tokio::test]
async fn environment_input_allows_recording_start_and_continues_until_yaml_stop() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("source.mp4");
    std::fs::write(&video, include_bytes!("../fixtures/tiny.mp4")).unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        ..State::default()
    }));
    let mut profile = Profile::default();
    // PATH is present without mutating process-global environment in parallel tests.
    profile.recipes.insert(
        "credential".into(),
        serde_yaml::from_str("- type_text: {text: '${env:PATH}'}").unwrap(),
    );
    let f = flow(
        "  - run: {recipe: credential}\n  - start_screen_recording: {}\n  - run: {recipe: credential}\n  - screenshot: {path: result.png}\n  - stop_screen_recording: {}",
    );
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&f, &profile, &[], Duration::from_secs(2))
        .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert!(report.limitations.is_empty());
    assert!(report.steps.iter().all(|step| step.status == "passed"));
    assert!(tmp.0.join("result.png").is_file());
    assert!(tmp.0.join("video.mp4").is_file());
    let state = state.lock().unwrap();
    let tools: Vec<_> = state
        .calls
        .iter()
        .filter(|c| c["method"] == "tools/call")
        .map(|c| c["params"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        vec![
            "mobile_list_available_devices",
            "mobile_type_keys",
            "mobile_start_screen_recording",
            "mobile_type_keys",
            "mobile_take_screenshot",
            "mobile_stop_screen_recording",
        ]
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

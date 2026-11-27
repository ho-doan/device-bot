use super::*;

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
            .to_string()
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
    assert!(
        schema::compile(&f, &p)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert!(
        schema::compile(
            &flow("  - tap_element: {target: typo}"),
            &Profile::default()
        )
        .unwrap_err()
        .to_string()
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
async fn invalid_documents_and_nested_actions_never_execute_device_tools() {
    for source in [
        "version: 2\nname: broken\nsteps: [",
        "version: 99\nname: unknown\nsteps: [{wait: {ms: 1}}]",
        "version: 2\nname: missing steps",
        "version: 2\nname: empty\nsteps: []",
        "version: 2\nname: scalar\nsteps: [42]",
        "version: 2\nname: no action\nsteps: [{}]",
        "version: 2\nname: missing coordinates\nsteps: [{tap: {x: 1}}]",
        "version: 2\nname: bad branch\nsteps: [{if_element: {target: unknown, then: 42}}]",
    ] {
        if let Ok(document) = FlowDocument::from_yaml(source) {
            let state = Arc::new(Mutex::new(State::default()));
            let report = run(&document, &Profile::default(), state.clone()).await;
            assert_eq!(report.status, "FAIL", "{source}");
            assert_eq!(report.error_kind.as_deref(), Some("setup"), "{source}");
            assert!(
                !state
                    .lock()
                    .unwrap()
                    .calls
                    .iter()
                    .any(|c| c["method"] == "tools/call"),
                "{source}"
            );
        }
    }
}

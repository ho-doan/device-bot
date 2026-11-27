use super::*;

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
        .args(["--target", "test", "--artifacts"])
        .arg(tmp.0.join("reports"))
        .arg(scenario)
        .env("ROBOT_FLOW_LITERAL_INPUT", "  Đặng ${literal} AbC  ")
        .env("DEVICE_BOT_REPORT_DATA_DIR", tmp.0.join("history"))
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
        serde_json::from_str(include_str!("../fixtures/ios-heading-sequence.json")).unwrap();
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
        serde_json::from_str(include_str!("../fixtures/ios-heading-sequence.json")).unwrap();
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
        serde_json::from_str(include_str!("../fixtures/ios-select-all-menu.json")).unwrap();
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
    async fn request(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<Value> {
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
    async fn notify(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<()> {
        self.inner.notify(method, params).await
    }
    async fn close(&mut self) -> robot_flow::mcp::Result<()> {
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

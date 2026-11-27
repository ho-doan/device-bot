use super::*;

#[tokio::test]
async fn named_resources_route_nested_scopes_and_restore_default() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([node("ready")])]),
        ..State::default()
    }));
    let profile: Profile = serde_yaml::from_str("selectors:\n  ready: {label: ready}\nrecipes:\n  nested:\n    - tap: {x: 1, y: 1}\n    - resource: desktop\n      tap: {x: 2, y: 2}\n    - tap: {x: 3, y: 3}\n").unwrap();
    let f = flow(
        "  - resource: alice\n    if_element:\n      target: ready\n      then:\n        - resource: alice\n          run: {recipe: nested}\n        - resource: bob\n          tap: {x: 4, y: 4}\n        - tap: {x: 5, y: 5}\n  - tap: {x: 6, y: 6}\n",
    );
    let tmp = Temp::new();
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone());
    runner.set_named_resources(bindings());
    let report = runner.run(&f, &profile, &[], Duration::from_secs(3)).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    assert_eq!(report.selected_device.unwrap()["id"], "device-1");
    let s = state.lock().unwrap();
    let ids: Vec<_> = s
        .calls
        .iter()
        .filter(|c| c["params"]["name"] == "mobile_click_on_screen_at_coordinates")
        .map(|c| c["params"]["arguments"]["deviceId"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "chrome-alice",
            "native",
            "chrome-alice",
            "chrome-bob",
            "chrome-alice",
            "device-1"
        ]
    );
}

#[tokio::test]
async fn emits_step_boundary_events_with_resource_and_platform_scope() {
    let state = Arc::new(Mutex::new(State::default()));
    let flow = FlowDocument::from_yaml(
        "version: 2\nname: events\nsteps:\n  - id: browser-step\n    resource: alice\n    tap: {x: 1, y: 2}\n  - id: platform-step\n    platform: android\n    tap: {x: 3, y: 4}\n  - id: native-step\n    resource: desktop\n    tap: {x: 5, y: 6}\n  - id: default-step\n    wait: {ms: 0}\n",
    )
    .unwrap();
    let tmp = Temp::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state))), tmp.0.clone());
    runner.set_named_resources(bindings());
    runner.set_default_device(json!({"id":"browser-default","platform":"chrome"}));
    let observed = Arc::clone(&events);
    runner.set_step_event_sink(Arc::new(move |event| {
        observed.lock().unwrap().push(event);
    }));

    let report = runner
        .run(&flow, &Profile::default(), &[], Duration::from_secs(3))
        .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);

    let events = events.lock().unwrap();
    assert_eq!(events.len(), 8);
    let started: Vec<_> = events
        .iter()
        .filter(|event| event.phase == robot_flow::FlowStepPhase::Started)
        .collect();
    assert_eq!(
        started
            .iter()
            .map(|event| event.step_id.as_str())
            .collect::<Vec<_>>(),
        [
            "browser-step",
            "platform-step",
            "native-step",
            "default-step"
        ]
    );
    assert_eq!(started[0].resource.as_deref(), Some("alice"));
    assert_eq!(started[0].platform, None);
    assert_eq!(started[1].resource, None);
    assert_eq!(started[1].platform.as_deref(), Some("android"));
    assert_eq!(started[2].resource.as_deref(), Some("desktop"));
    assert_eq!(started[3].resource, None);
    assert_eq!(started[3].platform, None);
    assert!(
        events
            .iter()
            .all(|event| event.phase != robot_flow::FlowStepPhase::Failed)
    );
}

#[tokio::test]
async fn emits_failed_step_boundary_when_action_fails() {
    let state = Arc::new(Mutex::new(State {
        fail: Some("mobile_click_on_screen_at_coordinates".into()),
        ..State::default()
    }));
    let flow = FlowDocument::from_yaml(
        "version: 2\nname: failed-event\nsteps:\n  - id: failing-step\n    tap: {x: 1, y: 2}\n",
    )
    .unwrap();
    let tmp = Temp::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state))), tmp.0.clone());
    runner.set_default_device(json!({"id":"default-device","platform":"android"}));
    let observed = Arc::clone(&events);
    runner.set_step_event_sink(Arc::new(move |event| {
        observed.lock().unwrap().push(event);
    }));

    let report = runner
        .run(&flow, &Profile::default(), &[], Duration::from_secs(3))
        .await;
    assert_eq!(report.status, "FAIL");
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].phase, robot_flow::FlowStepPhase::Started);
    assert_eq!(events[1].phase, robot_flow::FlowStepPhase::Failed);
    assert_eq!(events[1].step_id, "failing-step");
    assert_eq!(events[1].action, "tap");
}

#[tokio::test]
async fn missing_resource_in_unreachable_branch_fails_before_actions() {
    let state = Arc::new(Mutex::new(State::default()));
    let profile: Profile = serde_yaml::from_str("selectors:\n  ready: {label: ready}\n").unwrap();
    let f = flow(
        "  - if_element:\n      target: ready\n      then: []\n      else:\n        - resource: missing\n          tap: {x: 1, y: 1}\n",
    );
    let tmp = Temp::new();
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone());
    runner.set_named_resources(bindings());
    let report = runner.run(&f, &profile, &[], Duration::from_secs(3)).await;
    assert!(
        report
            .error
            .unwrap()
            .contains("unknown step resource missing")
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

#[tokio::test]
async fn deadline_cleans_recording_on_originating_resource() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("source.mp4");
    std::fs::write(&video, include_bytes!("../fixtures/tiny.mp4")).unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        ..State::default()
    }));
    let f = flow(
        "  - resource: alice\n    start_screen_recording: {}\n  - resource: desktop\n    wait: {ms: 500}\n",
    );
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.join("artifacts"));
    runner.set_named_resources(bindings());
    let report = runner
        .run(&f, &Profile::default(), &[], Duration::from_millis(100))
        .await;
    assert_eq!(report.error_kind.as_deref(), Some("timeout"));
    assert_eq!(report.selected_device.unwrap()["id"], "device-1");
    let s = state.lock().unwrap();
    let stop = s
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_stop_screen_recording")
        .unwrap();
    assert_eq!(stop["params"]["arguments"]["deviceId"], "chrome-alice");
}

#[tokio::test]
async fn scoped_action_failure_cleans_recording_on_originating_resource() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("source.mp4");
    std::fs::write(&video, include_bytes!("../fixtures/tiny.mp4")).unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        fail: Some("mobile_click_on_screen_at_coordinates".into()),
        ..State::default()
    }));
    let f = flow(
        "  - resource: alice\n    start_screen_recording: {}\n  - resource: desktop\n    tap: {x: 1, y: 2}\n",
    );
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.join("artifacts"));
    runner.set_named_resources(bindings());
    let report = runner
        .run(&f, &Profile::default(), &[], Duration::from_secs(3))
        .await;
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.selected_device.unwrap()["id"], "device-1");
    let s = state.lock().unwrap();
    let stop = s
        .calls
        .iter()
        .find(|c| c["params"]["name"] == "mobile_stop_screen_recording")
        .unwrap();
    assert_eq!(stop["params"]["arguments"]["deviceId"], "chrome-alice");
}

#[test]
fn resource_metadata_rejects_conflicts_and_invalid_names() {
    for metadata in ["resource: ''", "resource: alice\n    platform: chrome"] {
        let f = flow(&format!("  - {metadata}\n    tap: {{x: 1, y: 1}}\n"));
        assert!(schema::compile(&f, &Profile::default()).is_err());
    }
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

use super::*;

#[tokio::test]
async fn selector_pointer_gestures_send_one_typed_call_per_fresh_match() {
    let mut hold_only = node("Press");
    hold_only["clickable"] = json!(false);
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([hold_only]), json!([node("Open")])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - long_press_element: {label: Press, duration_ms: 925}\n  - double_tap_element: {label: Open}\n"),
        &Profile::default(), state.clone(),
    ).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let inputs: Vec<_> = state
        .calls
        .iter()
        .filter(|call| call["params"]["name"] == "mobile_click_on_screen_at_coordinates")
        .map(|call| call["params"]["arguments"].clone())
        .collect();
    assert_eq!(
        inputs,
        [
            json!({"deviceId":"device-1","x":60.0,"y":50.0,"gesture":"long_press","durationMs":925}),
            json!({"deviceId":"device-1","x":60.0,"y":50.0,"gesture":"double_tap"}),
        ]
    );
}

#[tokio::test]
async fn swipe_element_uses_current_bounds_or_fresh_dom_reference() {
    let mut native = node("Inbox");
    native["clickable"] = json!(false);
    let mut dom = node("Web row");
    let context = json!({"elementRef":"new-document-node","framePath":[{"css":"#inbox"}]});
    dom["webContext"] = context.clone();
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([native]), json!([dom])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - swipe_element: {label: Inbox, direction: left}\n  - swipe_element: {label: Web row, direction: right, duration_ms: 420, web_context: {frame_path: [{css: '#inbox'}]}}\n"),
        &Profile::default(), state.clone(),
    ).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let state = state.lock().unwrap();
    let args: Vec<_> = state
        .calls
        .iter()
        .filter(|call| call["params"]["name"] == "mobile_swipe_on_screen")
        .map(|call| call["params"]["arguments"].clone())
        .collect();
    assert_eq!(
        args,
        [
            json!({"deviceId":"device-1","mode":"drag","startX":100.0,"startY":50.0,"endX":20.0,"endY":50.0,"durationMs":300}),
            json!({"deviceId":"device-1","mode":"drag","startX":0.1,"startY":0.5,"endX":0.9,"endY":0.5,"durationMs":420,"webContext":context}),
        ]
    );
}

#[tokio::test]
async fn invalid_or_ambiguous_swipe_element_never_sends_drag() {
    for args in [
        "{label: Row}",
        "{label: Row, direction: up}",
        "{label: Row, direction: left, duration_ms: 3001}",
        "{label: Row, direction: right, native_tap: true}",
        "{label: Row, direction: left}",
    ] {
        let state = Arc::new(Mutex::new(State {
            queries: VecDeque::from([json!([node("Row"), node("Row")])]),
            ..State::default()
        }));
        let report = run(
            &flow(&format!("  - swipe_element: {args}\n")),
            &Profile::default(),
            state.clone(),
        )
        .await;
        assert_ne!(report.status, "PASS", "{args}");
        assert!(
            !state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|call| call["params"]["name"] == "mobile_swipe_on_screen")
        );
    }
}

#[tokio::test]
async fn invalid_or_ambiguous_pointer_gestures_never_send_input() {
    for yaml in [
        "  - long_press_element: {label: A, duration_ms: 0}\n",
        "  - double_tap_element: {label: A, duration_ms: 500}\n",
        "  - double_tap_element: {label: A, native_tap: true}\n",
        "  - long_press_element: {label: A, timeout_ms: 200}\n",
    ] {
        let state = Arc::new(Mutex::new(State {
            queries: VecDeque::from([json!([node("A"), node("A")])]),
            ..State::default()
        }));
        let report = run(&flow(yaml), &Profile::default(), state.clone()).await;
        assert_ne!(report.status, "PASS", "{yaml}");
        assert!(
            !state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|call| call["params"]["name"] == "mobile_click_on_screen_at_coordinates")
        );
    }
}

#[tokio::test]
async fn list_elements_probes_counts_timing_and_first_match_without_actions() {
    let state = Arc::new(Mutex::new(State {
        queries: VecDeque::from([json!([]), json!([node("First"), node("Second")]), json!([])]),
        ..State::default()
    }));
    let report = run(
        &flow("  - list_elements: {element_type: button, samples: 2, interval_ms: 50}\n  - list_elements: {element_type: textField}\n"),
        &Profile::default(),
        state.clone(),
    ).await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let report = serde_json::to_value(report).unwrap();
    let buttons = &report["steps"][1]["element_probe"];
    let samples = buttons["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0]["count"], 0);
    assert_eq!(samples[1]["count"], 2);
    assert!(samples[0]["query_ms"].is_u64());
    assert!(samples[1]["elapsed_ms"].as_u64().unwrap() >= 50);
    assert_eq!(buttons["first_match_ms"], samples[1]["elapsed_ms"]);
    let inputs = &report["steps"][2]["element_probe"];
    assert_eq!(inputs["samples"][0]["count"], 0);
    assert!(inputs["first_match_ms"].is_null());
    assert!(report["steps"][0].get("element_probe").is_none());
    let state = state.lock().unwrap();
    let names: Vec<_> = state
        .calls
        .iter()
        .filter_map(|call| call["params"]["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "mobile_list_available_devices",
            "mobile_list_elements_on_screen",
            "mobile_list_elements_on_screen",
            "mobile_list_elements_on_screen",
        ]
    );
}

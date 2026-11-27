use super::*;

fn web_fault_fixture(actions: &str) -> (FlowDocument, Arc<Mutex<State>>) {
    let flow = FlowDocument::from_yaml(&format!(
        "version: 2\nname: web-fault\nsteps:\n  - select_device: {{platform: chrome}}\n{actions}"
    ))
    .unwrap();
    let state = Arc::new(Mutex::new(State {
        devices: Some(json!([{"id":"chrome:owned","platform":"chrome","state":"available"}])),
        ..State::default()
    }));
    (flow, state)
}

#[tokio::test]
async fn web_fault_evidence_contains_revision_and_hits_but_not_request_path() {
    let (flow, state) = web_fault_fixture(
        "  - network_fault: {action: set, fault: {kind: http_status, status: 503, path: /private-data}}\n  - list_elements: {element_type: button}\n  - network_fault: {action: status, min_hits: 1}\n  - network_fault: {action: clear}\n",
    );
    let result = run(&flow, &Profile::default(), state.clone()).await;
    assert_eq!(result.status, "PASS", "{:?}", result.error);
    let ser = serde_json::to_value(result).unwrap();
    let steps = ser["steps"].as_array().unwrap();
    assert_eq!(steps[1]["network_fault"]["operation"], "set");
    assert_eq!(steps[1]["network_fault"]["active"]["kind"], "http_status");
    assert_eq!(steps[3]["network_fault"]["active"]["hits"], 1);
    assert_eq!(steps[4]["network_fault"]["previous"]["hits"], 1);
    assert_eq!(steps[4]["network_fault"]["operation"], "clear");
    assert!(steps[4]["network_fault"]["elapsed_ms"].is_u64());
    assert!(!ser.to_string().contains("private-data"));
    let s = state.lock().unwrap();
    assert!(s.web_fault.fault.is_none());
    let actions: Vec<&str> = s
        .calls
        .iter()
        .filter(|c| c["params"]["name"] == mcp::WEB_TEST_PROXY)
        .filter_map(|c| c["params"]["arguments"]["action"].as_str())
        .collect();
    assert_eq!(actions, ["set", "status", "clear"]);
}

#[tokio::test]
async fn no_matching_requests_fails_evidence_check_and_automatically_resets_the_fault() {
    let (flow, state) = web_fault_fixture(
        "  - network_fault: {action: set, fault: {kind: drop, path: /data}}\n  - network_fault: {action: status, min_hits: 1}\n",
    );
    let result = run(&flow, &Profile::default(), state.clone()).await;
    assert_eq!(result.status, "FAIL");
    assert_eq!(result.error_kind.as_deref(), Some("assertion"));
    assert!(result.error.unwrap().contains("observed 0"));
    let state = state.lock().unwrap();
    assert!(state.web_fault.fault.is_none());
    assert_eq!(state.web_fault.revision, 2);
    assert_eq!(
        state.calls.last().unwrap()["params"]["arguments"]["action"],
        "clear"
    );
}

use super::*;

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
                .to_string()
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

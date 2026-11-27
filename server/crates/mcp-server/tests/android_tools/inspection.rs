use super::*;

#[tokio::test]
async fn flutter_discovery_only_runs_for_missing_native_matches() {
    let robot = Arc::new(RecordingRobot::default());
    let mut server = Server::new(robot.clone());
    ready(&mut server).await;
    for (index, (arguments, source, flutter_calls, count)) in [
        (
            json!({"deviceId":"emulator-5554","filter":"Submit"}),
            "native",
            0,
            1,
        ),
        // All native nodes are returned, so ambiguity remains the caller's decision.
        (json!({"deviceId":"emulator-5554"}), "native", 0, 3),
        (
            json!({"deviceId":"emulator-5554","filter":"Flutter clear"}),
            "flutter",
            1,
            1,
        ),
        (
            json!({"deviceId":"emulator-5554","filter":"Flutter clear","source":"native"}),
            "native",
            1,
            0,
        ),
        (
            json!({"deviceId":"emulator-5554","source":"flutter"}),
            "flutter",
            2,
            1,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let response = server
            .handle(request(
                100 + index as u64,
                "tools/call",
                json!({
                    "name":"mobile_list_elements_on_screen", "arguments": arguments,
                }),
            ))
            .await
            .unwrap();
        let result = &response["result"]["structuredContent"];
        assert_eq!(result["source"], source);
        assert_eq!(result["elements"].as_array().unwrap().len(), count);
        assert_eq!(robot.actions.lock().unwrap().len(), flutter_calls);
    }
    let response = server
        .handle(request(
            200,
            "tools/call",
            json!({
                "name":"mobile_list_elements_on_screen", "arguments":{"deviceId":"native-error"},
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(robot.actions.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn locked_device_tool_errors_expose_actionable_recovery_data() {
    let mut server = Server::new(Arc::new(RecordingRobot::default()));
    ready(&mut server).await;
    let response = server
        .handle(request(
            800,
            "tools/call",
            json!({
                "name":"mobile_take_screenshot", "arguments":{"deviceId":"locked"}
            }),
        ))
        .await
        .unwrap();
    let result = &response["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["state"], "device_locked");
    assert_eq!(result["structuredContent"]["deviceId"], "locked");
    assert_eq!(
        result["structuredContent"]["recommendedActions"][0],
        "unlock_device"
    );
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unlock")
    );
}

#[tokio::test]
async fn element_position_is_one_based_and_applied_before_result_limit() {
    let robot = Arc::new(RecordingRobot::default());
    let mut server = Server::new(robot);
    ready(&mut server).await;
    for (i, (position, expected)) in [
        (json!("first"), Some(1)),
        (json!(2), Some(2)),
        (json!("last"), Some(3)),
        (json!(4), None),
    ]
    .into_iter()
    .enumerate()
    {
        let response = server
            .handle(request(
                300 + i as u64,
                "tools/call",
                json!({
                    "name": "mobile_list_elements_on_screen",
                    "arguments": {"deviceId":"emulator-5554", "position":position, "maxResults":1}
                }),
            ))
            .await
            .unwrap();
        let elements = response["result"]["structuredContent"]["elements"]
            .as_array()
            .unwrap();
        if let Some(index) = expected {
            assert_eq!(elements.len(), 1);
            assert_eq!(elements[0]["index"], index);
        } else {
            assert!(elements.is_empty());
        }
    }
    for position in [json!(0), json!(-1), json!("second"), Value::Null] {
        let response = server
            .handle(request(
                400,
                "tools/call",
                json!({
                    "name":"mobile_list_elements_on_screen",
                    "arguments":{"deviceId":"emulator-5554", "position":position}
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
    }
}

#[tokio::test]
async fn running_app_and_window_selection_is_independent_of_activation() {
    let robot = Arc::new(RecordingRobot {
        running_apps: Mutex::new(vec![RunningApp {
            process_id: 42,
            name: "Calculator".into(),
            bundle_id: None,
        }]),
        windows: Mutex::new(vec![AppWindow {
            window_id: "7".into(),
            process_id: 42,
            title: "Calc".into(),
            focused: true,
        }]),
        ..RecordingRobot::default()
    });
    let mut server = Server::new(robot.clone());
    ready(&mut server).await;

    let apps = server
        .handle(request(
            2,
            "tools/call",
            json!({
                "name": "mobile_list_running_apps",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        apps["result"]["structuredContent"]["apps"][0]["processId"],
        42
    );

    let windows = server
        .handle(request(
            3,
            "tools/call",
            json!({
                "name": "mobile_list_windows",
                "arguments": { "deviceId": "emulator-5554", "processId": 42 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        windows["result"]["structuredContent"]["windows"][0]["windowId"],
        "7"
    );

    let elements = server
        .handle(request(
            4,
            "tools/call",
            json!({
                "name": "mobile_list_elements_on_screen",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "processId": 42,
                    "windowId": "7"
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        elements["result"]["structuredContent"]["elements"][0]["text"],
        "Submit"
    );
    assert_eq!(
        robot.selected.lock().unwrap().as_slice(),
        ["emulator-5554:42:7"]
    );
    assert!(
        !robot
            .actions
            .lock()
            .unwrap()
            .iter()
            .any(|action| action.starts_with("activate:"))
    );

    let activated = server
        .handle(request(
            5,
            "tools/call",
            json!({
                "name": "mobile_activate_app",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "processId": 42,
                    "windowId": "7"
                }
            }),
        ))
        .await
        .unwrap();
    assert_ne!(activated["result"]["isError"], true);
    assert_eq!(
        robot.actions.lock().unwrap().last().unwrap(),
        "activate:emulator-5554:42:7"
    );
}

#[tokio::test]
async fn capability_tool_validates_arguments_and_redacts_driver_errors() {
    let mut server = Server::new(Arc::new(RecordingRobot::default()));
    ready(&mut server).await;
    for arguments in [
        json!({}),
        json!({"deviceId": "  "}),
        json!({"deviceId": "x", "extra": true}),
    ] {
        let result = server
            .handle(request(
                2,
                "tools/call",
                json!({"name":"mobile_get_capabilities", "arguments":arguments}),
            ))
            .await
            .unwrap();
        assert_eq!(result["result"]["isError"], true);
    }
    let result = server
        .handle(request(
            3,
            "tools/call",
            json!({"name":"mobile_get_capabilities", "arguments":{"deviceId":"failure"}}),
        ))
        .await
        .unwrap();
    assert_eq!(result["result"]["isError"], true);
    assert!(!result.to_string().contains("secret-token-from-driver"));
    let result = server
        .handle(request(
            4,
            "tools/call",
            json!({"name":"mobile_get_capabilities", "arguments":{"deviceId":"emulator-5554"}}),
        ))
        .await
        .unwrap();
    assert_eq!(
        result["result"]["structuredContent"]["hostOs"],
        std::env::consts::OS
    );
    assert_eq!(
        result["result"]["structuredContent"]["capabilities"]["input"]["status"],
        "unknown"
    );
}

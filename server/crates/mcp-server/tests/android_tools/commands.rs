use super::*;

#[tokio::test]
async fn app_termination_routes_foreground_explicit_and_all_requests() {
    let robot = Arc::new(RecordingRobot::default());
    let mut server = Server::new(robot.clone());
    ready(&mut server).await;
    for (id, name, arguments) in [
        (
            2,
            "mobile_terminate_app",
            json!({"deviceId":"emulator-5554"}),
        ),
        (
            3,
            "mobile_terminate_app",
            json!({"deviceId":"emulator-5554", "appId":"com.example.explicit"}),
        ),
        (
            4,
            "mobile_terminate_all_apps",
            json!({"deviceId":"emulator-5554"}),
        ),
    ] {
        let result = server
            .handle(request(
                id,
                "tools/call",
                json!({"name":name, "arguments":arguments}),
            ))
            .await
            .unwrap();
        assert_ne!(result["result"]["isError"], true, "{result}");
    }
    assert_eq!(
        *robot.actions.lock().unwrap(),
        [
            "terminate:emulator-5554:com.example.foreground",
            "terminate:emulator-5554:com.example.explicit",
            "terminate-all:emulator-5554",
        ]
    );
}

#[tokio::test]
async fn android_vertical_slice_exposes_and_routes_all_tools() {
    let robot = Arc::new(RecordingRobot::default());
    let mut server = Server::new(robot.clone());
    ready(&mut server).await;

    let catalogue = server
        .handle(request(2, "tools/list", json!({})))
        .await
        .unwrap();
    let tool_names: Vec<_> = catalogue["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tool_names,
        [
            "mobile_list_available_devices",
            "mobile_boot_device",
            "mobile_get_capabilities",
            "mobile_open_url",
            "mobile_type_keys",
            "mobile_press_ime_action",
            "mobile_list_elements_on_screen",
            "mobile_execute_ui_actions",
            "mobile_get_ui_runner_status",
            "mobile_terminate_app",
            "mobile_terminate_all_apps",
            "mobile_launch_app",
            "mobile_install_app",
            "mobile_uninstall_app",
            "mobile_list_installed_apps",
            "mobile_list_running_apps",
            "mobile_list_windows",
            "mobile_activate_app",
            "mobile_start_screen_recording",
            "mobile_stop_screen_recording",
            "mobile_read_screen_recording_chunk",
            "mobile_start_stream",
            "mobile_stop_stream",
            "mobile_get_viewport",
            "mobile_scroll",
            "mobile_list_displays",
            "mobile_take_screenshot",
            "mobile_click_on_screen_at_coordinates",
            "mobile_swipe_on_screen",
            "mobile_start_observation",
            "mobile_poll_observation",
            "mobile_stop_observation",
            "project_validate",
            "project_start",
            "project_status",
            "project_cancel",
            "mobile_web_test_proxy",
            "mobile_run_flow",
            "mobile_ui_map"
        ]
    );

    let devices = server
        .handle(request(
            3,
            "tools/call",
            json!({ "name": "mobile_list_available_devices", "arguments": {} }),
        ))
        .await
        .unwrap();
    assert_eq!(
        devices["result"]["structuredContent"]["devices"][0]["id"],
        "emulator-5554"
    );

    let apps = server
        .handle(request(
            4,
            "tools/call",
            json!({
                "name": "mobile_list_installed_apps",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(apps["result"]["structuredContent"]["platform"], "android");
    assert_eq!(apps["result"]["structuredContent"]["apps"], json!([]));

    let screenshot = server
        .handle(request(
            4,
            "tools/call",
            json!({
                "name": "mobile_take_screenshot",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(screenshot["result"]["content"][0]["type"], "image");
    assert_eq!(screenshot["result"]["content"][0]["data"], "AQID");

    let tap = server
        .handle(request(
            5,
            "tools/call",
            json!({
                "name": "mobile_click_on_screen_at_coordinates",
                "arguments": { "deviceId": "emulator-5554", "x": 12.5, "y": 99.5 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(tap["result"]["content"][0]["text"], "Tap completed");
    assert_eq!(
        robot.taps.lock().unwrap().as_slice(),
        [("emulator-5554".into(), Point { x: 12.5, y: 99.5 })]
    );

    let launch = server
        .handle(request(
            6,
            "tools/call",
            json!({
                "name": "mobile_launch_app",
                "arguments": { "deviceId": "emulator-5554", "appId": "com.android.chrome" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        launch["result"]["content"][0]["text"],
        "Launched com.android.chrome"
    );

    let opened = server
        .handle(request(
            7,
            "tools/call",
            json!({
                "name": "mobile_open_url",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "url": "https://www.youtube.com",
                    "appId": "com.android.chrome"
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(opened["result"]["isError"], true);
    assert!(
        opened["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("only for a chrome:<target-id>")
    );

    let typed = server
        .handle(request(
            8,
            "tools/call",
            json!({
                "name": "mobile_type_keys",
                "arguments": { "deviceId": "emulator-5554", "text": "babyshake" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        typed["result"]["content"][0]["text"],
        "Text input completed"
    );
    assert_eq!(
        robot.actions.lock().unwrap().as_slice(),
        [
            "launch:emulator-5554:com.android.chrome",
            "type:emulator-5554:babyshake"
        ]
    );

    let elements = server
        .handle(request(
            9,
            "tools/call",
            json!({
                "name": "mobile_list_elements_on_screen",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        elements["result"]["structuredContent"]["elements"][0]["text"],
        "Submit"
    );

    let filtered_elements = server
        .handle(request(
            15,
            "tools/call",
            json!({
                "name": "mobile_list_elements_on_screen",
                "arguments": { "deviceId": "emulator-5554", "filter": "sUbMiT" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        filtered_elements["result"]["structuredContent"]["elements"][0]["text"],
        "Submit"
    );

    let typed_elements = server
        .handle(request(
            16,
            "tools/call",
            json!({
                "name": "mobile_list_elements_on_screen",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "elementType": "textField",
                    "maxResults": 1
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        typed_elements["result"]["structuredContent"]["elements"][0]["resourceId"],
        "com.example:id/password"
    );
    assert_eq!(
        typed_elements["result"]["structuredContent"]["elements"][0]["elementType"],
        "textField"
    );
    assert_eq!(
        typed_elements["result"]["structuredContent"]["elements"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let invalid_type = server
        .handle(request(
            17,
            "tools/call",
            json!({
                "name": "mobile_list_elements_on_screen",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "elementType": "EditText"
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_type["result"]["isError"], true);
    assert!(
        invalid_type["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unsupported elementType")
    );

    let ime = server
        .handle(request(
            18,
            "tools/call",
            json!({
                "name": "mobile_press_ime_action",
                "arguments": { "deviceId": "emulator-5554", "action": "next" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        ime["result"]["content"][0]["text"],
        "Pressed IME action next"
    );

    let invalid_ime = server
        .handle(request(
            19,
            "tools/call",
            json!({
                "name": "mobile_press_ime_action",
                "arguments": { "deviceId": "emulator-5554", "action": "enter" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_ime["result"]["isError"], true);
    assert!(
        invalid_ime["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unsupported IME action")
    );

    let swipe = server
        .handle(request(
            10,
            "tools/call",
            json!({
                "name": "mobile_swipe_on_screen",
                "arguments": {
                    "deviceId": "emulator-5554",
                    "startX": 10,
                    "startY": 20,
                    "endX": 30,
                    "endY": 40,
                    "durationMs": 500
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(swipe["result"]["content"][0]["text"], "Swipe completed");

    let install = server
        .handle(request(
            11,
            "tools/call",
            json!({
                "name": "mobile_install_app",
                "arguments": { "deviceId": "emulator-5554", "apkPath": "/tmp/example.apk" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        install["result"]["content"][0]["text"],
        "Installed /tmp/example.apk"
    );

    let uninstall = server
        .handle(request(
            12,
            "tools/call",
            json!({
                "name": "mobile_uninstall_app",
                "arguments": { "deviceId": "emulator-5554", "appId": "com.example.app" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        uninstall["result"]["content"][0]["text"],
        "Uninstalled com.example.app"
    );
    assert_eq!(
        &robot.actions.lock().unwrap()[2..],
        [
            "ime:emulator-5554:next",
            "swipe:emulator-5554:10,20,30,40:500",
            "install:emulator-5554:/tmp/example.apk",
            "uninstall:emulator-5554:com.example.app"
        ]
    );

    let recording_started = server
        .handle(request(
            13,
            "tools/call",
            json!({
                "name": "mobile_start_screen_recording",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        recording_started["result"]["content"][0]["text"],
        "Screen recording started: artifacts/emulator-5554.mp4"
    );

    let recording_stopped = server
        .handle(request(
            14,
            "tools/call",
            json!({
                "name": "mobile_stop_screen_recording",
                "arguments": { "deviceId": "emulator-5554" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        recording_stopped["result"]["content"][0]["text"],
        format!(
            "Screen recording saved: {}",
            robot.recording_files.lock().unwrap()[0].display()
        )
    );
    let recording_id = recording_stopped["result"]["structuredContent"]["recordingId"]
        .as_str()
        .expect("finalized recording transfer ID");
    let chunk = server
        .handle(request(
            15,
            "tools/call",
            json!({
                "name": "mobile_read_screen_recording_chunk",
                "arguments": { "recordingId": recording_id, "offset": 0 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(chunk["result"]["structuredContent"]["offset"], 0);
    assert_eq!(chunk["result"]["structuredContent"]["size"], 3);
    assert_eq!(chunk["result"]["structuredContent"]["data"], "AQID");
    assert_eq!(
        &robot.actions.lock().unwrap()[6..],
        ["record-start:emulator-5554", "record-stop:emulator-5554"]
    );
    for path in robot.recording_files.lock().unwrap().drain(..) {
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn invalid_tool_arguments_are_tool_execution_errors() {
    let mut server = Server::new(Arc::new(RecordingRobot::default()));
    ready(&mut server).await;

    let result = server
        .handle(request(
            2,
            "tools/call",
            json!({ "name": "mobile_take_screenshot", "arguments": {} }),
        ))
        .await
        .unwrap();
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(
        result["result"]["content"][0]["text"],
        "Missing or invalid argument: deviceId"
    );
}

#[tokio::test]
async fn pixel_scroll_routes_signed_deltas_and_rejects_invalid_values() {
    let robot = Arc::new(RecordingRobot::default());
    let mut server = Server::new(robot.clone());
    ready(&mut server).await;
    for delta in [50, -50] {
        let response = server
            .handle(request(
                40,
                "tools/call",
                json!({
                    "name":"mobile_scroll", "arguments":{
                        "deviceId":"emulator-5554", "x":200, "y":300, "deltaX":0, "deltaY":delta
                    }
                }),
            ))
            .await
            .unwrap();
        assert_ne!(response["result"]["isError"], true);
    }
    assert_eq!(
        *robot.actions.lock().unwrap(),
        [
            "swipe:emulator-5554:200,300,200,250:200",
            "swipe:emulator-5554:200,300,200,350:200",
        ]
    );
    let response = server
        .handle(request(
            41,
            "tools/call",
            json!({
                "name":"mobile_scroll", "arguments":{
                    "deviceId":"emulator-5554", "x":200, "y":300, "deltaX":0, "deltaY":10001
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(robot.actions.lock().unwrap().len(), 2);
}

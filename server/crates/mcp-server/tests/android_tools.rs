use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use device_bot::Server;
use device_mcp_protocol::{JsonRpcRequest, PROTOCOL_VERSION};
use robot_core::{
    AppWindow, Bounds, DeviceState, ElementType, ImeAction, Platform, Point, Robot, RobotDevice,
    RobotError, RunningApp, UiElement, UiTarget,
};
use serde_json::{Value, json};

#[derive(Default)]
struct RecordingRobot {
    taps: Mutex<Vec<(String, Point)>>,
    actions: Mutex<Vec<String>>,
    running_apps: Mutex<Vec<RunningApp>>,
    windows: Mutex<Vec<AppWindow>>,
    selected: Mutex<Vec<String>>,
}

#[async_trait]
impl Robot for RecordingRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![RobotDevice {
            id: "emulator-5554".into(),
            name: "Pixel 9".into(),
            platform: Platform::Android,
            state: DeviceState::Available,
        }])
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        assert_eq!(device_id, "emulator-5554");
        Ok(json!({
            "platform": "android",
            "deviceId": device_id,
            "apps": [],
        }))
    }

    async fn list_running_apps(&self, device_id: &str) -> Result<Vec<RunningApp>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("running:{device_id}"));
        Ok(self.running_apps.lock().unwrap().clone())
    }

    async fn list_app_windows(
        &self,
        device_id: &str,
        process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("windows:{device_id}:{}", process_id.unwrap_or(0)));
        Ok(self
            .windows
            .lock()
            .unwrap()
            .iter()
            .filter(|window| process_id.is_none_or(|id| window.process_id == id))
            .cloned()
            .collect())
    }

    async fn select_ui_target(&self, device_id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.selected.lock().unwrap().push(format!(
            "{device_id}:{}:{}",
            target.process_id.unwrap_or(0),
            target.window_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn activate_ui_target(
        &self,
        device_id: &str,
        target: UiTarget,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "activate:{device_id}:{}:{}",
            target.process_id.unwrap_or(0),
            target.window_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        if device_id == "locked" {
            return Err(RobotError::DeviceLocked(device_id.into()));
        }
        if device_id == "emulator-5554" {
            Ok(vec![1, 2, 3])
        } else {
            Err(RobotError::DeviceNotFound(device_id.into()))
        }
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("record-start:{device_id}"));
        Ok(format!("artifacts/{device_id}.mp4"))
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("record-stop:{device_id}"));
        Ok(format!("artifacts/{device_id}.mp4"))
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.taps.lock().unwrap().push((device_id.into(), point));
        Ok(())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("launch:{device_id}:{app_id}"));
        Ok(())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "open:{device_id}:{url}:{}",
            app_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("type:{device_id}:{text}"));
        Ok(())
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("ime:{device_id}:{}", action.as_str()));
        Ok(())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "swipe:{device_id}:{},{},{},{}:{}",
            start.x,
            start.y,
            end.x,
            end.y,
            duration_ms.unwrap_or(300)
        ));
        Ok(())
    }

    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("install:{device_id}:{apk_path}"));
        Ok(())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("uninstall:{device_id}:{app_id}"));
        Ok(())
    }

    async fn list_flutter_elements(&self, _device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push("flutter-discovery".into());
        let mut node = self.list_elements("emulator-5554").await?.remove(0);
        node.resource_id = Some("flutter-only-key".into());
        node.text = Some("Flutter clear".into());
        node.value = Some(String::new());
        Ok(vec![node])
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        if device_id == "native-error" {
            return Err(RobotError::CommandFailed("native discovery failed".into()));
        }
        Ok(vec![
            UiElement {
                class_name: "android.widget.Button".into(),
                resource_id: Some("com.example:id/submit".into()),
                text: Some("Submit".into()),
                content_description: None,
                bounds: Bounds {
                    left: 10,
                    top: 20,
                    right: 100,
                    bottom: 70,
                },
                clickable: true,
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value: None,
                element_type: None,
            },
            UiElement {
                class_name: "android.widget.Button".into(),
                resource_id: None,
                text: Some("Cancel".into()),
                content_description: None,
                bounds: Bounds {
                    left: 110,
                    top: 20,
                    right: 200,
                    bottom: 70,
                },
                clickable: true,
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value: None,
                element_type: None,
            },
            UiElement {
                class_name: "android.widget.EditText".into(),
                resource_id: Some("com.example:id/password".into()),
                text: Some("secret".into()),
                content_description: None,
                bounds: Bounds {
                    left: 10,
                    top: 80,
                    right: 200,
                    bottom: 120,
                },
                clickable: true,
                enabled: true,
                focused: Some(true),
                checked: None,
                selected: None,
                value: Some("secret".into()),
                element_type: Some(ElementType::TextField),
            },
        ])
    }
}

fn request(id: u64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(id)),
        method: method.into(),
        params: Some(params),
    }
}

async fn ready(server: &mut Server) {
    server
        .handle(request(
            1,
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.1.0" }
            }),
        ))
        .await;
    server
        .handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        })
        .await;
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
            "mobile_open_url",
            "mobile_type_keys",
            "mobile_press_ime_action",
            "mobile_list_elements_on_screen",
            "mobile_execute_ui_actions",
            "mobile_get_ui_runner_status",
            "mobile_launch_app",
            "mobile_install_app",
            "mobile_uninstall_app",
            "mobile_list_installed_apps",
            "mobile_list_running_apps",
            "mobile_list_windows",
            "mobile_activate_app",
            "mobile_start_screen_recording",
            "mobile_stop_screen_recording",
            "mobile_start_stream",
            "mobile_stop_stream",
            "mobile_scroll",
            "mobile_list_displays",
            "mobile_take_screenshot",
            "mobile_click_on_screen_at_coordinates",
            "mobile_swipe_on_screen",
            "mobile_run_flow"
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
        "Screen recording saved: artifacts/emulator-5554.mp4"
    );
    assert_eq!(
        &robot.actions.lock().unwrap()[6..],
        ["record-start:emulator-5554", "record-stop:emulator-5554"]
    );
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

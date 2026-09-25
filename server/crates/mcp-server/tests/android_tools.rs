use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mobile_mcp_protocol::{JsonRpcRequest, PROTOCOL_VERSION};
use mobile_mcp_server::Server;
use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError};
use serde_json::{Value, json};

#[derive(Default)]
struct RecordingRobot {
    taps: Mutex<Vec<(String, Point)>>,
    actions: Mutex<Vec<String>>,
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

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        if device_id == "emulator-5554" {
            Ok(vec![1, 2, 3])
        } else {
            Err(RobotError::DeviceNotFound(device_id.into()))
        }
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
async fn android_vertical_slice_exposes_and_routes_all_six_tools() {
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
            "mobile_launch_app",
            "mobile_open_url",
            "mobile_type_keys",
            "mobile_take_screenshot",
            "mobile_click_on_screen_at_coordinates"
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
    assert_eq!(
        opened["result"]["content"][0]["text"],
        "Opened https://www.youtube.com"
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
            "open:emulator-5554:https://www.youtube.com:com.android.chrome",
            "type:emulator-5554:babyshake"
        ]
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

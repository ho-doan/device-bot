use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};

pub(super) const NAME: &str = "mobile_list_available_devices";
pub(super) const BOOT_NAME: &str = "mobile_boot_device";

pub(super) fn definitions() -> [ToolDefinition; 2] {
    [
        ToolDefinition {
            name: NAME.into(),
            title: Some("List discovered devices".into()),
            description: "List connected ADB devices, stopped Android AVDs (avd:<name>), iOS devices/simulators and Chrome CDP pages. An offline chrome:browser entry means the configured DevTools endpoint has no controllable page. Use mobile_boot_device for stopped virtual devices or Chrome, then refresh and use the discovered runtime device ID. state=available means detected/connected, not verified for UI control. For iOS, call mobile_get_ui_runner_status to check live helper identity/version and target app; use prepare=true only when setup/recovery is needed.".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
        },
        ToolDefinition {
            name: BOOT_NAME.into(),
            title: Some("Start a virtual device or Chrome".into()),
            description: "Start an iOS Simulator, Android AVD (avd:<name>) or local Chrome CDP browser (chrome:browser), and wait until ready. Refresh the device list afterwards: Android uses emulator-* IDs and Chrome uses chrome:<target-id> IDs for subsequent tools.".into(),
            input_schema: json!({"type":"object","properties":{"deviceId":{"type":"string","minLength":1}},"required":["deviceId"],"additionalProperties":false}),
        },
    ]
}

pub(super) async fn call(robot: &Arc<dyn Robot>) -> ToolResult {
    match robot.list_devices().await {
        Ok(devices) => ToolResult::structured(json!({ "devices": devices })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn boot(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let Some(device_id) = arguments
        .get("deviceId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return ToolResult::execution_error("Missing or invalid argument: deviceId");
    };
    match robot.boot_device(device_id).await {
        Ok(()) => ToolResult::text(format!("Booted {device_id}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

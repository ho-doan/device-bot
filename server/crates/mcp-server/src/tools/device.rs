use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

pub(super) const NAME: &str = "mobile_list_available_devices";
pub(super) const BOOT_NAME: &str = "mobile_boot_device";

pub(super) fn definitions() -> [ToolDefinition; 2] {
    [
        ToolDefinition {
            name: NAME.into(),
            title: Some("List available mobile devices".into()),
            description: "List Android and iOS devices known to the configured drivers.".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
        },
        ToolDefinition {
            name: BOOT_NAME.into(),
            title: Some("Boot a mobile simulator".into()),
            description: "Boot an iOS Simulator and wait until it is ready.".into(),
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

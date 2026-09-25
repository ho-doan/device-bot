use std::sync::Arc;

use base64::{Engine, engine::general_purpose::STANDARD};
use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::{Point, Robot};
use serde_json::{Value, json};

use super::arguments::{required_number, required_string};

pub(super) const SCREENSHOT_NAME: &str = "mobile_take_screenshot";
pub(super) const TAP_NAME: &str = "mobile_click_on_screen_at_coordinates";

pub(super) fn definitions() -> [ToolDefinition; 2] {
    [
        ToolDefinition {
            name: SCREENSHOT_NAME.into(),
            title: Some("Take a mobile screenshot".into()),
            description: "Capture the current screen as a PNG image.".into(),
            input_schema: device_id_schema(),
        },
        ToolDefinition {
            name: TAP_NAME.into(),
            title: Some("Tap mobile screen coordinates".into()),
            description: "Tap a screen coordinate on the selected device.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "x": { "type": "number" },
                    "y": { "type": "number" }
                },
                "required": ["deviceId", "x", "y"],
                "additionalProperties": false
            }),
        },
    ]
}

pub(super) async fn screenshot(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(device_id) => device_id,
        Err(error) => return error,
    };
    match robot.take_screenshot(&device_id).await {
        Ok(bytes) => ToolResult::image(STANDARD.encode(bytes), "image/png"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn tap(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(device_id) => device_id,
        Err(error) => return error,
    };
    let x = match required_number(arguments, "x") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let y = match required_number(arguments, "y") {
        Ok(value) => value,
        Err(error) => return error,
    };

    match robot.tap(&device_id, Point { x, y }).await {
        Ok(()) => ToolResult::text("Tap completed"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

fn device_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "deviceId": { "type": "string", "minLength": 1 }
        },
        "required": ["deviceId"],
        "additionalProperties": false
    })
}

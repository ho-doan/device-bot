use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const START_NAME: &str = "mobile_start_screen_recording";
pub(super) const STOP_NAME: &str = "mobile_stop_screen_recording";

pub(super) fn definitions() -> [ToolDefinition; 2] {
    [
        ToolDefinition {
            name: START_NAME.into(),
            title: Some("Start device screen recording".into()),
            description: "Start recording a supported device screen to a video artifact.".into(),
            input_schema: device_id_schema(),
        },
        ToolDefinition {
            name: STOP_NAME.into(),
            title: Some("Stop device screen recording".into()),
            description: "Stop the device recording and return its video artifact path.".into(),
            input_schema: device_id_schema(),
        },
    ]
}

pub(super) async fn start(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.start_screen_recording(&device_id).await {
        Ok(path) => ToolResult::text(format!("Screen recording started: {path}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn stop(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.stop_screen_recording(&device_id).await {
        Ok(path) => ToolResult::text(format!("Screen recording saved: {path}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

fn device_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "deviceId": { "type": "string", "minLength": 1 } },
        "required": ["deviceId"],
        "additionalProperties": false
    })
}

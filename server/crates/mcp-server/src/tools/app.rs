use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const LAUNCH_NAME: &str = "mobile_launch_app";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LAUNCH_NAME.into(),
        title: Some("Launch a mobile app".into()),
        description: "Launch an installed app by Android package identifier.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "appId": { "type": "string", "minLength": 3 }
            },
            "required": ["deviceId", "appId"],
            "additionalProperties": false
        }),
    }
}

pub(super) async fn launch(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let app_id = match required_string(arguments, "appId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.launch_app(&device_id, &app_id).await {
        Ok(()) => ToolResult::text(format!("Launched {app_id}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const TYPE_KEYS_NAME: &str = "mobile_type_keys";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: TYPE_KEYS_NAME.into(),
        title: Some("Type text on a mobile device".into()),
        description: "Type text into the currently focused field on the selected device.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "text": { "type": "string", "minLength": 1 }
            },
            "required": ["deviceId", "text"],
            "additionalProperties": false
        }),
    }
}

pub(super) async fn type_keys(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let text = match required_string(arguments, "text") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.type_text(&device_id, &text).await {
        Ok(()) => ToolResult::text("Text input completed"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

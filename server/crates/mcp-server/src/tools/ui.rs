use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const LIST_ELEMENTS_NAME: &str = "mobile_list_elements_on_screen";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_ELEMENTS_NAME.into(),
        title: Some("List accessibility elements on screen".into()),
        description:
            "Return the current Android accessibility tree normalized as interactive elements."
                .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 }
            },
            "required": ["deviceId"],
            "additionalProperties": false
        }),
    }
}

pub(super) async fn list_elements(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.list_elements(&device_id).await {
        Ok(elements) => ToolResult::structured(json!({ "elements": elements })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

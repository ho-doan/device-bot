use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const LIST_INSTALLED_APPS_NAME: &str = "mobile_list_installed_apps";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_INSTALLED_APPS_NAME.into(),
        title: Some("List installed apps".into()),
        description: "List installed apps for an Android, macOS, iOS, or Windows device.".into(),
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

pub(super) async fn list_apps(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.list_installed_apps(&device_id).await {
        Ok(output) => ToolResult::structured(output),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

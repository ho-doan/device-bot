use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const OPEN_URL_NAME: &str = "mobile_open_url";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: OPEN_URL_NAME.into(),
        title: Some("Navigate a Chrome DevTools page".into()),
        description: "Navigate an existing chrome:<target-id> page through Chrome DevTools. This is not a platform URL-opening operation.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "url": { "type": "string", "format": "uri" }
            },
            "required": ["deviceId", "url"],
            "additionalProperties": false
        }),
    }
}

pub(super) async fn open_url(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let url = match required_string(arguments, "url") {
        Ok(value) => value,
        Err(error) => return error,
    };
    if !device_id.starts_with("chrome:") {
        return ToolResult::execution_error(
            "mobile_open_url is supported only for a chrome:<target-id> device; launch Chrome with mobile_launch_app first",
        );
    }

    match robot.open_url(&device_id, &url, None).await {
        Ok(()) => ToolResult::text(format!("Opened {url}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

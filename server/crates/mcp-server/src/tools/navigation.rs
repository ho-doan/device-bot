use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const OPEN_URL_NAME: &str = "mobile_open_url";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: OPEN_URL_NAME.into(),
        title: Some("Open a URL on a mobile device".into()),
        description:
            "Open an HTTP(S) URL, optionally restricting it to an installed browser package.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "url": { "type": "string", "format": "uri" },
                "appId": { "type": "string", "minLength": 3 }
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
    let app_id = arguments
        .get("appId")
        .map(|_| required_string(arguments, "appId"))
        .transpose();
    let app_id = match app_id {
        Ok(value) => value,
        Err(error) => return error,
    };

    match robot.open_url(&device_id, &url, app_id.as_deref()).await {
        Ok(()) => ToolResult::text(format!("Opened {url}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

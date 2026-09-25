use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::json;

pub(super) const NAME: &str = "mobile_list_available_devices";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("List available mobile devices".into()),
        description: "List Android and future iOS devices known to the configured drivers.".into(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false
        }),
    }
}

pub(super) async fn call(robot: &Arc<dyn Robot>) -> ToolResult {
    match robot.list_devices().await {
        Ok(devices) => ToolResult::structured(json!({ "devices": devices })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

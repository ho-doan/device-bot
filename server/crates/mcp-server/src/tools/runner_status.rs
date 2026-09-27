use super::arguments::required_string;
use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};
use std::sync::Arc;
pub(super) const NAME: &str = "mobile_get_ui_runner_status";
pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("Get UI runner build status".into()),
        description: "Reports whether the iOS XCTest cache is ready for test-without-building."
            .into(),
        input_schema: json!({"type":"object","properties":{"deviceId":{"type":"string","minLength":1}},"required":["deviceId"],"additionalProperties":false}),
    }
}
pub(super) async fn get(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.ui_runner_status(&device_id).await {
        Ok(status) => ToolResult::structured(json!(status)),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

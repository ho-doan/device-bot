use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{Robot, UiAction};
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const NAME: &str = "mobile_execute_ui_actions";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("Execute mobile UI actions in one session".into()),
        description: "Execute tap, type, and swipe actions in one driver session. On iOS Simulator this uses one XCTest invocation instead of one per action.".into(),
        input_schema: json!({"type":"object","properties":{"deviceId":{"type":"string","minLength":1},"actions":{"type":"array","minItems":1,"maxItems":50,"items":{"type":"object"}}},"required":["deviceId","actions"],"additionalProperties":false}),
    }
}

pub(super) async fn execute(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let Some(actions) = arguments.get("actions") else {
        return ToolResult::execution_error("Missing or invalid argument: actions");
    };
    let actions: Vec<UiAction> = match serde_json::from_value::<Vec<UiAction>>(actions.clone()) {
        Ok(value) if !value.is_empty() => value,
        _ => return ToolResult::execution_error("Missing or invalid argument: actions"),
    };
    match robot.execute_ui_actions(&device_id, &actions).await {
        Ok(()) => ToolResult::text(format!("Completed {} UI actions", actions.len())),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{ImeAction, Robot};
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const TYPE_KEYS_NAME: &str = "mobile_type_keys";
pub(super) const PRESS_IME_ACTION_NAME: &str = "mobile_press_ime_action";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: TYPE_KEYS_NAME.into(),
        title: Some("Type text on a mobile device".into()),
        description: "Type text into the currently focused field. Set clear=true to clear the focused field instead; text may be empty for clear.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "text": { "type": "string" }, "clear": { "type": "boolean" }
            },
            "required": ["deviceId", "text"],
            "additionalProperties": false
        }),
    }
}

pub(super) fn press_ime_definition() -> ToolDefinition {
    ToolDefinition {
        name: PRESS_IME_ACTION_NAME.into(),
        title: Some("Press a named IME action".into()),
        description: "Press one IME action on the focused editor: next, done, go, search, or send. These actions are distinct. Android reads the focused editor's imeOptions and sends KEYCODE_ENTER only when that action matches; a mismatch is an error and does not press Enter. iOS taps the keyboard key with that exact label and does not fall back to another key.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "action": {
                    "type": "string",
                    "enum": ["next", "done", "go", "search", "send"]
                }
            },
            "required": ["deviceId", "action"],
            "additionalProperties": false
        }),
    }
}

pub(super) async fn type_keys(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    if arguments.get("clear").and_then(Value::as_bool) == Some(true) {
        return match robot.clear_text(&device_id).await {
            Ok(()) => ToolResult::text("Focused field cleared"),
            Err(e) => ToolResult::execution_error(e.to_string()),
        };
    }
    let text = match required_string(arguments, "text") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.type_text(&device_id, &text).await {
        Ok(()) => ToolResult::text("Text input completed"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn press_ime_action(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let action = match required_string(arguments, "action") {
        Ok(value) => match ImeAction::parse(&value) {
            Ok(action) => action,
            Err(error) => return ToolResult::execution_error(error.to_string()),
        },
        Err(error) => return error,
    };
    match robot.press_ime_action(&device_id, action).await {
        Ok(()) => ToolResult::text(format!("Pressed IME action {}", action.as_str())),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

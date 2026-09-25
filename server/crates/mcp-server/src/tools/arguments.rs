use serde_json::Value;

use mobile_mcp_protocol::ToolResult;

pub(super) fn required_string(arguments: &Value, key: &str) -> Result<String, ToolResult> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ToolResult::execution_error(format!("Missing or invalid argument: {key}")))
}

pub(super) fn required_number(arguments: &Value, key: &str) -> Result<f64, ToolResult> {
    arguments
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| ToolResult::execution_error(format!("Missing or invalid argument: {key}")))
}

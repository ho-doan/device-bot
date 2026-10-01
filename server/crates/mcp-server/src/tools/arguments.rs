use serde_json::Value;

use device_mcp_protocol::ToolResult;

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

pub(super) fn optional_u64(arguments: &Value, key: &str) -> Result<Option<u64>, ToolResult> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            ToolResult::execution_error(format!("Missing or invalid argument: {key}"))
        }),
    }
}

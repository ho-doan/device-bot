use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::{optional_u64, required_string};

pub(super) const LIST_ELEMENTS_NAME: &str = "mobile_list_elements_on_screen";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_ELEMENTS_NAME.into(),
        title: Some("List accessibility elements on screen".into()),
        description: "Return normalized accessibility elements. `filter` is applied by the platform driver and `maxResults` bounds the response.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "filter": { "type": "string", "minLength": 1 },
                "maxResults": { "type": "integer", "minimum": 1, "maximum": 100 }
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
    let filter = arguments
        .get("filter")
        .map(|_| required_string(arguments, "filter"))
        .transpose();
    let filter = match filter {
        Ok(value) => value,
        Err(error) => return error,
    };
    let max_results = match optional_u64(arguments, "maxResults") {
        Ok(Some(value @ 1..=100)) => Some(value as usize),
        Ok(Some(_)) => return ToolResult::execution_error("maxResults must be between 1 and 100"),
        Ok(None) => None,
        Err(error) => return error,
    };
    match robot
        .list_elements_filtered(&device_id, filter.as_deref(), max_results)
        .await
    {
        Ok(elements) => ToolResult::structured(json!({ "elements": elements })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

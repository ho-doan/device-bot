use std::{sync::Arc, time::Duration};

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{ElementQuery, ElementType, Robot, prepare_elements};
use serde_json::{Value, json};

use super::arguments::{optional_u64, required_string};

pub(super) const LIST_ELEMENTS_NAME: &str = "mobile_list_elements_on_screen";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_ELEMENTS_NAME.into(),
        title: Some("List accessibility elements on screen".into()),
        description: "Return normalized accessibility elements. Optional `filter` and `elementType` are combined with AND and applied before `maxResults`. `elementType` is one of textField, button, link, staticText, checkBox, switch, image, radio, or slider. textField includes password, search, and multiline editors. An unknown type is an error, not an empty element list. `focused`, `checked`, and `selected` are omitted when the driver cannot observe them; omission means unknown, not false. `value` is the native editor value when the platform exposes one.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "filter": { "type": "string", "minLength": 1 },
                "elementType": {
                    "type": "string",
                    "enum": ["textField", "button", "link", "staticText", "checkBox", "switch", "image", "radio", "slider"]
                },
                "maxResults": { "type": "integer", "minimum": 1, "maximum": 100 },
                "timeoutMs": { "type": "integer", "minimum": 1, "maximum": 60000 }
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
    let element_type = match arguments.get("elementType") {
        None | Some(serde_json::Value::Null) => None,
        Some(_) => match required_string(arguments, "elementType") {
            Ok(value) => match ElementType::parse(&value) {
                Ok(element_type) => Some(element_type),
                Err(error) => return ToolResult::execution_error(error.to_string()),
            },
            Err(error) => return error,
        },
    };
    let max_results = match optional_u64(arguments, "maxResults") {
        Ok(Some(value @ 1..=100)) => Some(value as usize),
        Ok(Some(_)) => return ToolResult::execution_error("maxResults must be between 1 and 100"),
        Ok(None) => None,
        Err(error) => return error,
    };
    let timeout = match optional_u64(arguments, "timeoutMs") {
        Ok(Some(value @ 1..=60_000)) => Some(Duration::from_millis(value)),
        Ok(Some(_)) => return ToolResult::execution_error("timeoutMs must be between 1 and 60000"),
        Ok(None) => None,
        Err(error) => return error,
    };
    let list = robot.list_elements_query(
        &device_id,
        ElementQuery {
            filter,
            element_type,
            max_results,
        },
    );
    let result = match timeout {
        Some(timeout) => match tokio::time::timeout(timeout, list).await {
            Ok(result) => result,
            Err(_) => {
                return ToolResult::execution_error(format!(
                    "mobile_list_elements_on_screen timed out after {}ms",
                    timeout.as_millis()
                ));
            }
        },
        None => list.await,
    };
    match result {
        Ok(mut elements) => {
            prepare_elements(&mut elements);
            ToolResult::structured(json!({ "elements": elements }))
        }
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

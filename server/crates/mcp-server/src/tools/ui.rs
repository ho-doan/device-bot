use std::{sync::Arc, time::Duration};

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{
    ElementQuery, ElementType, Robot, WebContext, apply_element_query, prepare_elements,
};
use serde_json::{Value, json};

use super::arguments::{optional_u64, required_string};

pub(super) const LIST_ELEMENTS_NAME: &str = "mobile_list_elements_on_screen";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_ELEMENTS_NAME.into(),
        title: Some("List accessibility elements on screen".into()),
        description: "Return normalized accessibility elements with a one-based `index` in query tree order (snapshot-local, not a persistent ID). `position` selects first, last, or a one-based match after filtering. Use its bounds for tap, typing focus, or scroll gestures. On Chrome pages, the driver automatically enables Flutter Web accessibility when its standard opt-in placeholder is present. source=auto queries native first and uses an opt-in Flutter bridge only when native has no matches. source=native or flutter selects one tree; native errors and ambiguous matches are never hidden. Optional `filter` and `elementType` are combined with AND and applied before `maxResults`. `elementType` is one of textField, button, link, staticText, checkBox, switch, image, radio, or slider. textField includes password, search, and multiline editors. An unknown type is an error, not an empty element list. `focused`, `checked`, and `selected` are omitted when the driver cannot observe them; omission means unknown, not false. `value` is the native editor value when the platform exposes one. On macOS/Windows, optional `processId` and `windowId` select a running app without activating it; use `mobile_activate_app` to bring that target to the foreground.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "deviceId": { "type": "string", "minLength": 1 },
                "filter": { "type": "string", "minLength": 1 },
                "elementType": {
                    "type": "string",
                    "enum": ["textField", "button", "link", "staticText", "checkBox", "switch", "image", "radio", "slider"]
                },
                "source": { "type": "string", "enum": ["auto", "native", "flutter"], "default": "auto" },
                "position": { "description": "One-based position in tree order after type/text filtering, before maxResults. Use first, last, or a positive integer. Positions belong to this snapshot only.", "oneOf": [{ "type": "integer", "minimum": 1 }, { "type": "string", "enum": ["first", "last"] }] },
                "maxResults": { "type": "integer", "minimum": 1, "maximum": 100 },
                "timeoutMs": { "type": "integer", "minimum": 1, "maximum": 60000 },
                "processId": { "type": "integer", "minimum": 1, "description": "Running process to query. Desktop only. Does not activate the app." },
                "windowId": { "type": "string", "minLength": 1, "description": "Window from mobile_list_windows. Desktop only." },
                "includeFrames": { "type": "boolean", "description": "Include descendant iframe and WebView documents. Omit it to keep unscoped queries on the root document." },
                "webContext": { "type": "object", "description": "Stable app, page, and frame path. Runtime CDP ids are not accepted.", "additionalProperties": false, "properties": {
                    "appId": { "type": "string" },
                    "page": { "type": "object", "additionalProperties": false, "properties": { "url": { "type": "string" }, "title": { "type": "string" } } },
                    "framePath": { "type": "array", "items": { "type": "object", "additionalProperties": false, "properties": { "css": { "type": "string" }, "name": { "type": "string" }, "url": { "type": "string" }, "position": { "type": "integer", "minimum": 1 } } } }
                }}
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
    let position = arguments.get("position");
    if position.is_some_and(|v| {
        v.as_u64().is_none_or(|n| n == 0) && !matches!(v.as_str(), Some("first" | "last"))
    }) {
        return ToolResult::execution_error(
            "position must be first, last, or a positive one-based integer",
        );
    }
    let timeout = match optional_u64(arguments, "timeoutMs") {
        Ok(Some(value @ 1..=60_000)) => Some(Duration::from_millis(value)),
        Ok(Some(_)) => return ToolResult::execution_error("timeoutMs must be between 1 and 60000"),
        Ok(None) => None,
        Err(error) => return error,
    };
    let target = match super::apps::ui_target(arguments) {
        Ok(target) => target,
        Err(error) => return error,
    };
    if target.process_id.is_some() || target.window_id.is_some() {
        if let Err(error) = robot.select_ui_target(&device_id, target).await {
            return ToolResult::execution_error(error.to_string());
        }
    }
    let source = arguments
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    if !["auto", "native", "flutter"].contains(&source)
        || arguments
            .get("source")
            .is_some_and(|value| !value.is_string())
    {
        return ToolResult::execution_error("source must be auto, native, or flutter");
    }
    let web_context = match arguments.get("webContext") {
        None | Some(Value::Null) => None,
        Some(value) => match serde_json::from_value::<WebContext>(value.clone()) {
            Ok(context) => match context.validate() {
                Ok(()) => Some(context),
                Err(error) => return ToolResult::execution_error(error.to_string()),
            },
            Err(error) => {
                return ToolResult::execution_error(format!("invalid webContext: {error}"));
            }
        },
    };
    if web_context
        .as_ref()
        .is_some_and(|context| !context.is_empty())
        && source == "flutter"
    {
        return ToolResult::execution_error("webContext cannot be combined with source flutter");
    }
    let include_frames = match arguments.get("includeFrames") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return ToolResult::execution_error("includeFrames must be a boolean"),
    };
    let query = ElementQuery {
        filter,
        element_type,
        max_results: if position.is_some() {
            Some(100)
        } else {
            max_results
        },
        web_context,
        include_frames,
    };
    let list = async {
        if source != "flutter" {
            let elements = robot.list_elements_query(&device_id, query.clone()).await?;
            if source == "native" || !elements.is_empty() {
                return Ok::<_, robot_core::RobotError>((elements, "native"));
            }
        }
        let mut elements = robot.list_flutter_elements(&device_id).await?;
        prepare_elements(&mut elements);
        Ok((apply_element_query(elements, &query), "flutter"))
    };
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
        Ok((mut elements, source)) => {
            prepare_elements(&mut elements);
            if position.is_some() && elements.len() >= 100 {
                return ToolResult::execution_error(
                    "positional query reached 100 elements; narrow the filter to avoid a truncated selection",
                );
            }
            let count = elements.len();
            let selected = position.map(|v| match v.as_str() {
                Some("first") => 0,
                Some("last") => count.saturating_sub(1),
                _ => usize::try_from(v.as_u64().unwrap() - 1).unwrap_or(usize::MAX),
            });
            let elements: Vec<Value> = elements
                .into_iter()
                .enumerate()
                .filter(|(index, _)| selected.is_none_or(|selected| selected == *index))
                .map(|(index, element)| {
                    let mut value = serde_json::to_value(element).unwrap();
                    value["index"] = json!(index + 1);
                    value
                })
                .collect();
            ToolResult::structured(json!({ "elements": elements, "source": source }))
        }
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

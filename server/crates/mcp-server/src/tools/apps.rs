use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{Robot, UiTarget};
use serde_json::{Value, json};

use super::arguments::{optional_u64, required_string};

const MAX_PROCESS_ID: u64 = u32::MAX as u64;

pub(super) const LIST_INSTALLED_APPS_NAME: &str = "mobile_list_installed_apps";
pub(super) const LIST_RUNNING_APPS_NAME: &str = "mobile_list_running_apps";
pub(super) const LIST_WINDOWS_NAME: &str = "mobile_list_windows";
pub(super) const ACTIVATE_APP_NAME: &str = "mobile_activate_app";

pub(super) fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: LIST_INSTALLED_APPS_NAME.into(),
            title: Some("List installed apps".into()),
            description: "List installed apps for an Android, macOS, iOS, or Windows device.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 }
                },
                "required": ["deviceId"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: LIST_RUNNING_APPS_NAME.into(),
            title: Some("List running apps".into()),
            description: "List processes that currently have windows on a macOS or Windows host. Distinct from installed apps. Use a processId with mobile_list_windows, mobile_list_elements_on_screen, or mobile_activate_app. Element listing does not activate the target.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 }
                },
                "required": ["deviceId"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: LIST_WINDOWS_NAME.into(),
            title: Some("List app windows".into()),
            description: "List windows for a running desktop process. Omit processId to list every on-screen window. windowId is passed to mobile_list_elements_on_screen or mobile_activate_app.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "processId": { "type": "integer", "minimum": 1 }
                },
                "required": ["deviceId"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: ACTIVATE_APP_NAME.into(),
            title: Some("Activate running app".into()),
            description: "Bring a running macOS/Windows process, and optionally one of its windows, to the foreground. Separate from listing accessibility elements, which can query a background PID.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "processId": { "type": "integer", "minimum": 1 },
                    "windowId": { "type": "string", "minLength": 1 }
                },
                "required": ["deviceId", "processId"],
                "additionalProperties": false
            }),
        },
    ]
}

pub(super) async fn list_apps(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.list_installed_apps(&device_id).await {
        Ok(output) => ToolResult::structured(output),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn list_running_apps(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.list_running_apps(&device_id).await {
        Ok(apps) => ToolResult::structured(json!({ "deviceId": device_id, "apps": apps })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn list_windows(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let process_id = match optional_u64(arguments, "processId") {
        Ok(Some(value @ 1..=MAX_PROCESS_ID)) => Some(value as u32),
        Ok(Some(_)) => {
            return ToolResult::execution_error("processId must be a positive 32-bit integer");
        }
        Ok(None) => None,
        Err(error) => return error,
    };
    match robot.list_app_windows(&device_id, process_id).await {
        Ok(windows) => ToolResult::structured(json!({ "deviceId": device_id, "windows": windows })),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) fn ui_target(arguments: &Value) -> Result<UiTarget, ToolResult> {
    let process_id = match optional_u64(arguments, "processId") {
        Ok(Some(value @ 1..=MAX_PROCESS_ID)) => Some(value as u32),
        Ok(Some(_)) => {
            return Err(ToolResult::execution_error(
                "processId must be a positive 32-bit integer",
            ));
        }
        Ok(None) => None,
        Err(error) => return Err(error),
    };
    let window_id = match arguments.get("windowId") {
        None | Some(Value::Null) => None,
        Some(_) => Some(required_string(arguments, "windowId")?),
    };
    Ok(UiTarget {
        process_id,
        window_id,
    })
}

pub(super) async fn activate_app(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let target = match ui_target(arguments) {
        Ok(target) => target,
        Err(error) => return error,
    };
    if target.process_id.is_none() {
        return ToolResult::execution_error("Missing or invalid argument: processId");
    }
    match robot.activate_ui_target(&device_id, target).await {
        Ok(()) => ToolResult::text("Activated target app"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

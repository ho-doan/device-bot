use std::sync::Arc;

use mobile_mcp_protocol::{ToolDefinition, ToolResult};
use mobile_robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const LAUNCH_NAME: &str = "mobile_launch_app";
pub(super) const INSTALL_NAME: &str = "mobile_install_app";
pub(super) const UNINSTALL_NAME: &str = "mobile_uninstall_app";

pub(super) fn definitions() -> [ToolDefinition; 3] {
    [
        ToolDefinition {
            name: LAUNCH_NAME.into(),
            title: Some("Launch a mobile app".into()),
            description: "Launch an installed app by Android package identifier.".into(),
            input_schema: app_id_schema(),
        },
        ToolDefinition {
            name: INSTALL_NAME.into(),
            title: Some("Install an Android APK".into()),
            description: "Install or replace an APK available on the MCP server host.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "apkPath": { "type": "string", "minLength": 5, "description": "Host path to an APK file" }
                },
                "required": ["deviceId", "apkPath"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: UNINSTALL_NAME.into(),
            title: Some("Uninstall an Android app".into()),
            description: "Uninstall an app by Android package identifier.".into(),
            input_schema: app_id_schema(),
        },
    ]
}

pub(super) async fn launch(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let app_id = match required_string(arguments, "appId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.launch_app(&device_id, &app_id).await {
        Ok(()) => ToolResult::text(format!("Launched {app_id}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn install(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let apk_path = match required_string(arguments, "apkPath") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.install_app(&device_id, &apk_path).await {
        Ok(()) => ToolResult::text(format!("Installed {apk_path}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn uninstall(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let app_id = match required_string(arguments, "appId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.uninstall_app(&device_id, &app_id).await {
        Ok(()) => ToolResult::text(format!("Uninstalled {app_id}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

fn app_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "deviceId": { "type": "string", "minLength": 1 },
            "appId": { "type": "string", "minLength": 3 }
        },
        "required": ["deviceId", "appId"],
        "additionalProperties": false
    })
}

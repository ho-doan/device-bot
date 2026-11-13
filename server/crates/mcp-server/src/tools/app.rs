use std::sync::Arc;

use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};

use super::arguments::required_string;

pub(super) const LAUNCH_NAME: &str = "mobile_launch_app";
pub(super) const INSTALL_NAME: &str = "mobile_install_app";
pub(super) const UNINSTALL_NAME: &str = "mobile_uninstall_app";
pub(super) const TERMINATE_NAME: &str = "mobile_terminate_app";
pub(super) const TERMINATE_ALL_NAME: &str = "mobile_terminate_all_apps";

pub(super) fn definitions() -> [ToolDefinition; 5] {
    [
        ToolDefinition {
            name: TERMINATE_NAME.into(),
            title: Some("Force-stop an Android app".into()),
            description: "Force-stop an Android package for the current user. Omit appId to stop the foreground app.".into(),
            input_schema: json!({"type":"object", "properties":{
                "deviceId":{"type":"string", "minLength":1},
                "appId":{"type":"string", "minLength":3}
            }, "required":["deviceId"], "additionalProperties":false}),
        },
        ToolDefinition {
            name: TERMINATE_ALL_NAME.into(),
            title: Some("Force-stop all Android user apps".into()),
            description: "Force-stop all third-party installed Android apps for the current user. System apps are excluded. Does not uninstall apps or clear their data.".into(),
            input_schema: json!({"type":"object", "properties":{
                "deviceId":{"type":"string", "minLength":1}
            }, "required":["deviceId"], "additionalProperties":false}),
        },
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

pub(super) async fn terminate(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let app_id = if arguments.get("appId").is_some() {
        match required_string(arguments, "appId") {
            Ok(value) => value,
            Err(error) => return error,
        }
    } else {
        match robot.stream_context(&device_id).await {
            Ok(context) => match context["appId"].as_str().filter(|app| !app.is_empty()) {
                Some(app) => app.to_owned(),
                None => {
                    return ToolResult::execution_error(
                        "No foreground app was found. Specify appId to stop an app.",
                    );
                }
            },
            Err(error) => return ToolResult::execution_error(error.to_string()),
        }
    };
    match robot.terminate_app(&device_id, &app_id).await {
        Ok(()) => ToolResult::text(format!("Stopped {app_id}")),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn terminate_all(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot.terminate_all_apps(&device_id).await {
        Ok(apps) => ToolResult::text(format!("Stopped {} user apps", apps.len())),
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

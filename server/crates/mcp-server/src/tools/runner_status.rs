use super::arguments::required_string;
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};
use std::sync::Arc;
pub(super) const NAME: &str = "mobile_get_ui_runner_status";
pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("Check or prepare device UI helper".into()),
        description: "By default, checks the live helper identity, version, target app and build cache without building, installing or starting it. prepare=true builds only missing/outdated products and starts/installs the helper. reinstall=true with prepare=true restarts/reinstalls the helper using the current cached build; it never reboots the device OS. appId selects the target app. ready requires a compatible live helper for this exact device and a selected target app.".into(),
        input_schema: json!({"type":"object","properties":{
            "deviceId":{"type":"string","minLength":1},
            "prepare":{"type":"boolean","default":false},
            "reinstall":{"type":"boolean","default":false},
            "appId":{"type":"string","minLength":1}
        },"required":["deviceId"],"additionalProperties":false}),
    }
}
pub(super) async fn get(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    for key in ["prepare", "reinstall"] {
        if arguments.get(key).is_some_and(|v| !v.is_boolean()) {
            return ToolResult::execution_error(format!("{key} must be boolean"));
        }
    }
    let prepare = arguments["prepare"].as_bool().unwrap_or(false);
    let reinstall = arguments["reinstall"].as_bool().unwrap_or(false);
    let app_id = match arguments.get("appId") {
        None => None,
        Some(_) => match required_string(arguments, "appId") {
            Ok(value) => Some(value),
            Err(error) => return error,
        },
    };
    if !prepare && (reinstall || app_id.is_some()) {
        return ToolResult::execution_error("reinstall/appId require prepare=true");
    }
    let status = if !prepare {
        robot.check_device_health(&device_id).await
    } else if reinstall {
        robot.reboot_helper(&device_id, app_id.as_deref()).await
    } else {
        robot.prepare_helper(&device_id, app_id.as_deref()).await
    };
    match status {
        Ok(status) => {
            let mut value = json!(status);
            value["deviceId"] = json!(device_id);
            value["serverVersion"] = json!(env!("CARGO_PKG_VERSION"));
            ToolResult::structured(value)
        }
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

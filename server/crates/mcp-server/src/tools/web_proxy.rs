//! Session-scoped control plane for Chrome's test proxy.

use super::arguments::required_string;
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{Robot, WebFault, WebProxyConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) const NAME: &str = robot_flow::mcp::WEB_TEST_PROXY;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Configure { config: WebProxyConfig },
    Set { fault: WebFault },
    Clear {},
    Status {},
}

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("Control an isolated Chrome test proxy".into()),
        description: "Opt-in test-only proxy for an owned Chrome session. Configure its explicit loopback destinations before boot, then set, clear, or read a fault. Non-loopback destinations go direct. This is not a system proxy.".into(),
        input_schema: json!({"type":"object","properties":{
            "deviceId":{"type":"string","minLength":1},
            "action":{"type":"string","enum":["configure","set","clear","status"]},
            "config":{"type":"object","description":"Loopback upstream and optional loopback passthrough origins.",
                "properties":{"upstream":{"type":"string"},"passthrough":{"type":"array","items":{"type":"string"}}},
                "required":["upstream"],"additionalProperties":false},
            "fault":{"type":"object","description":"Fault kind and, for HTTP, the exact URL path.",
                "properties":{"kind":{"type":"string","enum":["http_status","delay","drop","web_socket_reject","web_socket_drop"]},
                    "path":{"type":"string"},"status":{"type":"integer"},"ms":{"type":"integer"}},
                "required":["kind"],"additionalProperties":false}
        },"required":["deviceId","action"],"additionalProperties":false}),
    }
}

pub(super) async fn call(robot: &Arc<dyn Robot>, args: &Value) -> ToolResult {
    let id = match required_string(args, "deviceId") {
        Ok(id) => id,
        Err(error) => return error,
    };
    if !id.starts_with("chrome:") {
        return ToolResult::execution_error("web test proxy requires a Chrome device or page");
    }
    let mut args = args.as_object().unwrap().clone();
    args.remove("deviceId");
    let Ok(command) = serde_json::from_value::<Command>(Value::Object(args)) else {
        return ToolResult::execution_error("invalid web test proxy action or arguments");
    };
    let result = match command {
        Command::Configure { config } => match config.validate() {
            Ok(()) => robot
                .configure_web_proxy(&id, config)
                .await
                .map(|()| json!({"configured":true})),
            Err(error) => Err(error),
        },
        Command::Set { fault } => match fault.validate() {
            Ok(()) => robot
                .set_web_fault(&id, Some(fault))
                .await
                .map(|state| json!(state)),
            Err(error) => Err(error),
        },
        Command::Clear {} => robot
            .set_web_fault(&id, None)
            .await
            .map(|state| json!(state)),
        Command::Status {} => robot.web_fault_state(&id).await.map(|state| json!(state)),
    };
    match result {
        Ok(value) => ToolResult::structured(value),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_actions_and_non_loopback_config_never_reach_a_driver() {
        let robot: Arc<dyn Robot> = Arc::new(robot_core::RobotRegistry::default());
        for args in [
            json!({"deviceId":"chrome:browser","action":"configure","config":{"upstream":"192.0.2.1:8080"}}),
            json!({"deviceId":"chrome:page","action":"clear","fault":{"kind":"web_socket_drop"}}),
            json!({"deviceId":"chrome:page","action":"set","fault":{"kind":"http_status","path":"/api","status":200}}),
            json!({"deviceId":"android:fixture","action":"status"}),
        ] {
            let reply = call(&robot, &args).await;
            assert_eq!(reply.is_error, Some(true));
            assert!(
                !format!("{:?}", reply.content).contains("device not found"),
                "{args:?}"
            );
        }
    }
}

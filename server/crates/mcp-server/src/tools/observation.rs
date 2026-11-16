use super::arguments::required_string;
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{ObservationOptions, Robot};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) const START: &str = "mobile_start_observation";
pub(super) const POLL: &str = "mobile_poll_observation";
pub(super) const STOP: &str = "mobile_stop_observation";
pub(super) fn definitions() -> Vec<ToolDefinition> {
    [(START, "Start page observation", json!({"deviceId":{"type":"string","minLength":1},"includeText":{"type":"boolean"},"redact":{"type":"array","maxItems":64,"items":{"type":"string","maxLength":1024}}}), vec!["deviceId"]),
     (POLL, "Poll page observation", json!({"deviceId":{"type":"string","minLength":1},"observationId":{"type":"string","minLength":1},"afterSequence":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":1024}}), vec!["deviceId","observationId"]),
     (STOP, "Stop page observation", json!({"deviceId":{"type":"string","minLength":1},"observationId":{"type":"string","minLength":1}}), vec!["deviceId","observationId"])]
    .into_iter().map(|(name,title,properties,required)| ToolDefinition { name:name.into(), title:Some(title.into()), description:"Observe a session-owned browser page; events are bounded and metadata-only by default.".into(), input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}) }).collect()
}
pub(super) async fn call(robot: &Arc<dyn Robot>, name: &str, args: &Value) -> ToolResult {
    let Some(object) = args.as_object() else {
        return ToolResult::execution_error("arguments must be an object");
    };
    let allowed: &[&str] = match name {
        START => &["deviceId", "includeText", "redact"],
        POLL => &["deviceId", "observationId", "afterSequence", "limit"],
        _ => &["deviceId", "observationId"],
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return ToolResult::execution_error("unknown observation argument");
    }
    if args.get("includeText").is_some_and(|v| !v.is_boolean())
        || args.get("redact").is_some_and(|v| {
            !v.as_array().is_some_and(|a| {
                a.len() <= 64
                    && a.iter()
                        .all(|s| s.as_str().is_some_and(|s| s.len() <= 1024))
            })
        })
    {
        return ToolResult::execution_error("invalid observation options");
    }
    if args
        .get("afterSequence")
        .is_some_and(|v| v.as_u64().is_none())
        || args
            .get("limit")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| (1..=1024).contains(&n)))
    {
        return ToolResult::execution_error("invalid observation cursor or limit");
    }
    let id = match required_string(args, "deviceId") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if name == START {
        let options = match serde_json::from_value::<ObservationOptions>(args.clone()) {
            Ok(v) => v,
            Err(e) => return ToolResult::execution_error(e.to_string()),
        };
        return match robot.start_observation(&id, options).await {
            Ok(v) => ToolResult::structured(json!({"observationId":v})),
            Err(e) => ToolResult::execution_error(e.to_string()),
        };
    }
    let observation = match required_string(args, "observationId") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if name == POLL {
        let after = args["afterSequence"].as_u64().unwrap_or(0);
        let limit = args["limit"].as_u64().unwrap_or(100).min(1024) as usize;
        match robot
            .poll_observation(&id, &observation, after, limit)
            .await
        {
            Ok(v) => ToolResult::structured(json!(v)),
            Err(e) => ToolResult::execution_error(e.to_string()),
        }
    } else {
        match robot.stop_observation(&id, &observation).await {
            Ok(()) => ToolResult::text("Observation stopped"),
            Err(e) => ToolResult::execution_error(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_invalid_inputs_before_routing() {
        let robot: Arc<dyn Robot> = Arc::new(robot_core::RobotRegistry::default());
        for (name, args) in [
            (START, json!({"deviceId":"x","includeText":"yes"})),
            (START, json!({"deviceId":"x","unexpected":true})),
            (
                POLL,
                json!({"deviceId":"x","observationId":"a","afterSequence":-1}),
            ),
            (POLL, json!({"deviceId":"x","observationId":"a","limit":0})),
            (STOP, json!({"deviceId":"x","observationId":"a","limit":1})),
        ] {
            let result = call(&robot, name, &args).await;
            assert_eq!(result.is_error, Some(true));
            assert!(!format!("{:?}", result.content).contains("device not found"));
        }
    }
}

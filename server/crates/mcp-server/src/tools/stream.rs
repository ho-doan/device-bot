use super::arguments::required_string;
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use robot_stream::StreamManager;
use serde_json::{Value, json};
use std::sync::Arc;
pub(super) const START_NAME: &str = "mobile_start_stream";
pub(super) const STOP_NAME: &str = "mobile_stop_stream";
pub(super) fn definitions() -> [ToolDefinition; 2] {
    [ToolDefinition { name: START_NAME.into(), title: Some("Start realtime video".into()),
        description: "Negotiate Android H.264 or Chrome browser-native WebRTC. Supply a gathered recvonly video SDP offer; returns streamId, SDP answer and device dimensions. Video travels over WebRTC, never MCP.".into(),
        input_schema: json!({"type":"object","properties":{"deviceId":{"type":"string","minLength":1},"sdp":{"type":"string","minLength":1}},"required":["deviceId","sdp"],"additionalProperties":false}) },
    ToolDefinition { name: STOP_NAME.into(), title: Some("Stop realtime video".into()),
        description: "Idempotently close this MCP session's WebRTC stream and Android capture.".into(),
        input_schema: json!({"type":"object","properties":{"streamId":{"type":"string","minLength":1}},"required":["streamId"],"additionalProperties":false}) }]
}
pub(super) async fn start(
    robot: &Arc<dyn Robot>,
    streams: &StreamManager,
    args: &Value,
) -> ToolResult {
    let id = match required_string(args, "deviceId") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sdp = match required_string(args, "sdp") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match streams.start(robot.as_ref(), &id, sdp).await {
        Ok(answer) => {
            let value = serde_json::to_value(answer).expect("stream answer");
            let mut result = ToolResult::text(value.to_string());
            result.structured_content = Some(value);
            result
        }
        Err(e) => ToolResult::execution_error(e.to_string()),
    }
}
pub(super) async fn stop(streams: &StreamManager, args: &Value) -> ToolResult {
    let id = match required_string(args, "streamId") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match streams.stop(&id).await {
        Ok(()) => ToolResult::text("Stream stopped"),
        Err(e) => ToolResult::execution_error(e.to_string()),
    }
}

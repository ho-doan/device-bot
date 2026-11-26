//! Shared MCP payload construction used by preflight and execution.

use crate::{
    mcp::Result,
    schema::{self, Selector},
};
use serde_json::{Value, json};

pub(super) fn query_arguments(s: &Selector, device: &str) -> Value {
    // The runner owns step/case deadlines and the transport owns its RPC deadline.
    // A separate fixed query limit would cut explicitly longer YAML waits short.
    let mut args = json!({"deviceId":device,"maxResults":s.max_results.unwrap_or(100)});
    if let Some(filter) = s
        .filter
        .as_ref()
        .or(s.label.as_ref())
        .or(s.resource_id.as_ref())
    {
        args["filter"] = json!(filter);
    }
    if let Some(t) = &s.element_type {
        args["elementType"] = json!(t);
    }
    if let Some(context) = s.web_context.as_ref().filter(|context| !context.is_empty()) {
        args["webContext"] = context.to_tool_value();
    }
    args
}
pub(super) fn type_arguments(device: &str, text: &str, args: &Value) -> Result<Value> {
    let mut payload = json!({"deviceId": device, "text": text});
    if let Some(value) = args.get("web_context") {
        let context: schema::FlowWebContext = serde_json::from_value(value.clone())
            .map_err(|error| format!("invalid web_context: {error}"))?;
        context.validate()?;
        if !context.is_empty() {
            payload["webContext"] = context.to_tool_value();
        }
    }
    Ok(payload)
}
pub(super) fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| crate::FlowError::from(format!("missing or empty {key}")))
}
pub(super) fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| crate::FlowError::from(format!("missing string {key}")))
}
pub(super) fn number(v: &Value, key: &str) -> Result<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| crate::FlowError::from(format!("invalid coordinate {key}")))
}
pub(super) fn tap_arguments(device: &str, element: &Value, b: (f64, f64, f64, f64)) -> Value {
    let mut args = json!({"deviceId":device,"x":(b.0+b.2)/2.,"y":(b.1+b.3)/2.});
    if let Some(context) = element.get("webContext").filter(|value| !value.is_null()) {
        args["webContext"] = context.clone();
    }
    args
}

pub(super) fn pointer_gesture_arguments(
    device: &str,
    element: &Value,
    bounds: (f64, f64, f64, f64),
    gesture: robot_core::PointerGesture,
) -> Value {
    let mut args = tap_arguments(device, element, bounds);
    args["gesture"] = json!(gesture.name());
    if let robot_core::PointerGesture::LongPress { duration_ms } = gesture {
        args["durationMs"] = json!(duration_ms);
    }
    args
}

pub(super) fn rectangle(v: &Value) -> Result<(f64, f64, f64, f64)> {
    let b = &v["bounds"];
    let r = (
        number(b, "left")?,
        number(b, "top")?,
        number(b, "right")?,
        number(b, "bottom")?,
    );
    if r.0 < 0. || r.1 < 0. || r.2 <= r.0 || r.3 <= r.1 {
        return Err("element has no visible valid bounds".into());
    }
    Ok(r)
}
pub(super) fn gesture_arguments(args: &Value, e: &Value, device: &str) -> Result<Value> {
    let b = rectangle(e)?;
    let x = |n: f64| b.0 + (b.2 - b.0) * n;
    let y = |n: f64| b.1 + (b.3 - b.1) * n;
    let (sx, sy, ex, ey) = (
        number(&args["from"], "x")?,
        number(&args["from"], "y")?,
        number(&args["to"], "x")?,
        number(&args["to"], "y")?,
    );
    if [sx, sy, ex, ey]
        .into_iter()
        .any(|n| !(0.0..=1.0).contains(&n))
    {
        return Err("gesture fractions must be 0..1".into());
    }
    Ok(
        json!({"deviceId":device,"startX":x(sx),"startY":y(sy),"endX":x(ex),"endY":y(ey),"durationMs":args.get("duration_ms").and_then(Value::as_u64).unwrap_or(350)}),
    )
}
pub(super) fn swipe_arguments(args: &Value, device: &str) -> Result<Value> {
    Ok(
        json!({"deviceId":device,"startX":number(&args["start"],"x")?,"startY":number(&args["start"],"y")?,"endX":number(&args["end"],"x")?,"endY":number(&args["end"],"y")?,"durationMs":args.get("duration_ms").and_then(Value::as_u64).unwrap_or(300)}),
    )
}

// A scoped platform is authoritative; never silently call a tool on a different device.
// The YAML launch contract accepts a native app identifier/path or a web URL.
// Keep preflight and execution on the same MCP tool and argument schema.
pub(super) fn app_launch_call(device_id: &str, app_id: &str) -> (&'static str, Value) {
    if app_id.starts_with("http://") || app_id.starts_with("https://") {
        (
            "mobile_open_url",
            json!({"deviceId": device_id, "url": app_id}),
        )
    } else {
        (
            "mobile_launch_app",
            json!({"deviceId": device_id, "appId": app_id}),
        )
    }
}

pub(super) fn route_tool_arguments(arguments: &mut Value, device_id: &str) -> Result<()> {
    let object = arguments
        .as_object_mut()
        .ok_or("call_tool.arguments must be a mapping")?;
    if let Some(value) = object.get("deviceId") {
        if value != "<selected>" && value != device_id {
            return Err("call_tool.arguments.deviceId conflicts with step platform; use <selected> or omit deviceId".into());
        }
    }
    object.insert("deviceId".into(), json!(device_id));
    Ok(())
}

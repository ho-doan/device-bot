//! Action-specific schema validation and decoded arguments.

use crate::mcp::Result;
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) fn validate_action_keys(action: &str, args: &Value) -> Result<()> {
    let selector = "target filter label labels element_type resource_id prefer_resource_id class_contains row_with exclude_row_with allow_missing_row_anchor largest native_tap stable_samples replace_recipe verify_recipe require_focus scroll_into_view max_results timeout_ms position source web_context";
    let extra = match action {
        "run" => "recipe with",
        "if_element" => "target then else",
        "select_device" => "id name platform state",
        "launch_app" => "app_id",
        "wait" => "ms",
        "screenshot" => "path",
        "tap" => "x y",
        "type_text" => "text sensitive web_context",
        "swipe" => "start end duration_ms",
        "call_tool" => "name arguments sensitive",
        "note_limitation" => "message",
        "network_fault" => "action fault min_hits timeout_ms",
        "start_screen_recording" | "stop_screen_recording" | "reset_fields" => "",
        "tap_element" | "double_tap_element" | "assert_element" | "assert_absent"
        | "focus_field" => selector,
        "long_press_element" => return check_keys(args, &format!("{selector} duration_ms")),
        "list_elements" => return check_keys(args, &format!("{selector} samples interval_ms")),
        "assert_state" => return check_keys(args, &format!("{selector} property expected")),
        "assert_value" => return check_keys(args, &format!("{selector} expected")),
        "assert_count" => return check_keys(args, &format!("{selector} count")),
        "fill_field" => {
            return check_keys(args, &format!("{selector} text text_from_env sensitive"));
        }
        "gesture" => return check_keys(args, &format!("{selector} from to duration_ms")),
        "swipe_element" => return check_keys(args, &format!("{selector} direction duration_ms")),
        "scroll" => return check_keys(args, &format!("{selector} direction wheel_delta")),
        _ => return Err(format!("unknown action {action}").into()),
    };
    check_keys(args, extra)
}

pub(crate) fn scroll_wheel_delta(args: &Value) -> Result<Option<f64>> {
    let direction = match args.get("direction") {
        None => 1.,
        Some(value) if value == "down" => 1.,
        Some(value) if value == "up" => -1.,
        _ => return Err("scroll direction must be up or down".into()),
    };
    args.get("wheel_delta")
        .map(|value| {
            let distance = value
                .as_u64()
                .filter(|distance| (1..=2000).contains(distance))
                .ok_or("scroll wheel_delta must be an integer in 1..2000")?;
            Ok(distance as f64 * direction)
        })
        .transpose()
}

pub(crate) fn pointer_gesture_step(
    action: &str,
    args: &Value,
) -> Result<robot_core::PointerGesture> {
    let name = match action {
        "long_press_element" => "long_press",
        "double_tap_element" => "double_tap",
        _ => return Err(format!("not a pointer gesture: {action}").into()),
    };
    let duration = args
        .get("duration_ms")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| crate::FlowError::from("duration_ms must be an integer"))
        })
        .transpose()?;
    robot_core::PointerGesture::parse(name, duration).map_err(|error| error.to_string().into())
}

pub(crate) fn swipe_element_step(args: &Value) -> Result<(bool, u64)> {
    let left = match args["direction"].as_str() {
        Some("left") => true,
        Some("right") => false,
        _ => return Err("swipe_element.direction must be left or right".into()),
    };
    let duration = match args.get("duration_ms") {
        None => robot_core::DEFAULT_DRAG_DURATION_MS,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=robot_core::MAX_DRAG_DURATION_MS).contains(n))
            .ok_or("swipe_element.duration_ms must be 1..3000")?,
    };
    Ok((left, duration))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NetworkFaultStep {
    Set { fault: robot_core::WebFault },
    Status { min_hits: Option<u64> },
    Clear {},
}

impl NetworkFaultStep {
    pub(crate) fn tool_arguments(&self, device: &str) -> Value {
        match self {
            Self::Set { fault } => json!({"deviceId":device,"action":"set","fault":fault}),
            Self::Status { .. } => json!({"deviceId":device,"action":"status"}),
            Self::Clear {} => json!({"deviceId":device,"action":"clear"}),
        }
    }
}

pub(crate) fn network_fault_step(args: &Value) -> Result<NetworkFaultStep> {
    let mut raw = args.clone();
    raw.as_object_mut()
        .ok_or("network_fault arguments must be an object")?
        .remove("timeout_ms");
    let step = serde_json::from_value::<NetworkFaultStep>(raw)
        .map_err(|_| "invalid network_fault action or arguments")?;
    if let NetworkFaultStep::Set { fault } = &step {
        fault.validate().map_err(|error| error.to_string())?;
    }
    Ok(step)
}
fn check_keys(args: &Value, allowed: &str) -> Result<()> {
    let map = args
        .as_object()
        .ok_or("action arguments must be a mapping")?;
    for key in map.keys() {
        if !allowed.split_whitespace().any(|k| k == key) {
            return Err(format!("unknown action argument {key}").into());
        }
    }
    Ok(())
}

use std::sync::Arc;

use base64::{Engine, engine::general_purpose::STANDARD};
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::{Point, Robot, ScreenshotOptions};
use serde_json::{Value, json};

use super::arguments::{optional_bool, optional_u64, required_number, required_string};

pub(super) const SCREENSHOT_NAME: &str = "mobile_take_screenshot";
pub(super) const DISPLAYS_NAME: &str = "mobile_list_displays";
pub(super) const TAP_NAME: &str = "mobile_click_on_screen_at_coordinates";
pub(super) const SWIPE_NAME: &str = "mobile_swipe_on_screen";

pub(super) fn definitions() -> [ToolDefinition; 4] {
    [
        ToolDefinition {
            name: DISPLAYS_NAME.into(), title: Some("List desktop displays".into()),
            description: "List macOS/Windows displays with IDs, primary status and global coordinate bounds. Refresh before capture when the monitor configuration changes.".into(),
            input_schema: device_id_schema(),
        },
        ToolDefinition {
            name: SCREENSHOT_NAME.into(),
            title: Some("Take a mobile screenshot".into()),
            description: "Capture a PNG. Desktop options: appId selects/activates the target app, displayId selects a display returned by mobile_list_displays. The previous foreground app (such as QC) is restored after capture, including failures. UI element coordinates remain global desktop coordinates. annotateElements draws each element's index and visible text on the screenshot, then widens the PNG with a YAML flow hint (resource_id, label, position, class_contains, element_type, source) for that element. The result's structuredContent.annotated is false when the element tree or image cannot be labeled; the raw PNG is still returned.".into(),
            input_schema: json!({"type":"object", "properties":{
                "deviceId":{"type":"string","minLength":1},
                "appId":{"type":"string","minLength":1},
                "displayId":{"type":"string","minLength":1},
                "annotateElements":{"type":"boolean","default":false}
            },"required":["deviceId"],"additionalProperties":false}),
        },
        ToolDefinition {
            name: TAP_NAME.into(),
            title: Some("Tap mobile screen coordinates".into()),
            description: "Tap a screen coordinate on the selected device.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "x": { "type": "number" },
                    "y": { "type": "number" }
                },
                "required": ["deviceId", "x", "y"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: SWIPE_NAME.into(),
            title: Some("Swipe across a mobile screen".into()),
            description: "Swipe between two screen coordinates; duration defaults to 300ms.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deviceId": { "type": "string", "minLength": 1 },
                    "startX": { "type": "number" },
                    "startY": { "type": "number" },
                    "endX": { "type": "number" },
                    "endY": { "type": "number" },
                    "durationMs": { "type": "integer", "minimum": 1, "maximum": 60000 }
                },
                "required": ["deviceId", "startX", "startY", "endX", "endY"],
                "additionalProperties": false
            }),
        },
    ]
}

pub(super) async fn screenshot(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(device_id) => device_id,
        Err(error) => return error,
    };
    let mut options = ScreenshotOptions::default();
    for (key, slot) in [
        ("appId", &mut options.app_id),
        ("displayId", &mut options.display_id),
    ] {
        if arguments.get(key).is_some() {
            match required_string(arguments, key) {
                Ok(value) => *slot = Some(value),
                Err(error) => return error,
            }
        }
    }
    let annotate = match optional_bool(arguments, "annotateElements") {
        Ok(value) => value,
        Err(error) => return error,
    };
    match robot
        .take_screenshot_with_options(&device_id, &options)
        .await
    {
        Ok(bytes) if annotate => {
            match device_admin_api::with_element_labels(
                robot.as_ref(),
                &device_id,
                bytes,
                options.display_id.as_deref(),
            )
            .await
            {
                Ok(shot) => ToolResult::labeled_image(
                    STANDARD.encode(shot.bytes),
                    "image/png",
                    shot.annotated,
                ),
                Err(error) => ToolResult::execution_error(error.to_string()),
            }
        }
        Ok(bytes) => ToolResult::image(STANDARD.encode(bytes), "image/png"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn displays(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let id = match required_string(arguments, "deviceId") {
        Ok(id) => id,
        Err(error) => return error,
    };
    match robot.list_displays(&id).await {
        Ok(displays) => ToolResult::structured(json!({"deviceId":id,"displays":displays})),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn tap(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(device_id) => device_id,
        Err(error) => return error,
    };
    let x = match required_number(arguments, "x") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let y = match required_number(arguments, "y") {
        Ok(value) => value,
        Err(error) => return error,
    };

    match robot.tap(&device_id, Point { x, y }).await {
        Ok(()) => ToolResult::text("Tap completed"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

pub(super) async fn swipe(robot: &Arc<dyn Robot>, arguments: &Value) -> ToolResult {
    let device_id = match required_string(arguments, "deviceId") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let start_x = match required_number(arguments, "startX") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let start_y = match required_number(arguments, "startY") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let end_x = match required_number(arguments, "endX") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let end_y = match required_number(arguments, "endY") {
        Ok(value) => value,
        Err(error) => return error,
    };
    let duration_ms = match optional_u64(arguments, "durationMs") {
        Ok(value) => value,
        Err(error) => return error,
    };

    match robot
        .swipe(
            &device_id,
            Point {
                x: start_x,
                y: start_y,
            },
            Point { x: end_x, y: end_y },
            duration_ms,
        )
        .await
    {
        Ok(()) => ToolResult::text("Swipe completed"),
        Err(error) => ToolResult::execution_error(error.to_string()),
    }
}

fn device_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "deviceId": { "type": "string", "minLength": 1 }
        },
        "required": ["deviceId"],
        "additionalProperties": false
    })
}

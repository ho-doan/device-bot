//! Decoded simctl inventory and XCTest UI payloads.

use std::collections::HashMap;

use robot_core::{Bounds, DeviceState, Platform, RobotDevice, RobotError, UiElement};
use serde::Deserialize;
use serde_json::Value;

pub(super) fn decode_agent_elements(result: &Value) -> Result<Vec<UiElement>, RobotError> {
    let elements: Vec<AgentElement> = serde_json::from_value(
        result
            .get("elements")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .map_err(|error| {
        RobotError::CommandFailed(format!(
            "invalid persistent XCTest element payload: {error}"
        ))
    })?;
    Ok(elements
        .into_iter()
        .map(AgentElement::into_ui_element)
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentElement {
    node_id: String,
    automation_id: Option<String>,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    role: String,
    bounds: AgentBounds,
    enabled: bool,
    clickable: bool,
    #[serde(default)]
    focused: Option<bool>,
    #[serde(default)]
    checked: Option<bool>,
    #[serde(default)]
    selected: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl AgentElement {
    fn into_ui_element(self) -> UiElement {
        let value = self.value.filter(|value| !value.is_empty());
        UiElement {
            class_name: self.role,
            resource_id: self.automation_id.or(Some(self.node_id)),
            text: self.text.or_else(|| value.clone()),
            content_description: self.label.filter(|value| !value.is_empty()),
            bounds: Bounds {
                left: self.bounds.x.round() as i32,
                top: self.bounds.y.round() as i32,
                right: (self.bounds.x + self.bounds.width).round() as i32,
                bottom: (self.bounds.y + self.bounds.height).round() as i32,
            },
            clickable: self.clickable,
            enabled: self.enabled,
            focused: self.focused,
            checked: self.checked,
            selected: self.selected,
            value,
            element_type: None,
            web_context: None,
        }
    }
}

#[derive(Deserialize)]
struct SimctlList {
    devices: HashMap<String, Vec<SimctlDevice>>,
}
#[derive(Deserialize)]
struct SimctlDevice {
    udid: String,
    name: String,
    state: String,
    #[serde(rename = "isAvailable")]
    available: Option<bool>,
}

pub fn parse_simctl_devices(json: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let list: SimctlList = serde_json::from_str(json).map_err(|error| {
        RobotError::CommandFailed(format!("could not parse simctl devices: {error}"))
    })?;
    Ok(list
        .devices
        .into_values()
        .flatten()
        .filter(|device| device.available.unwrap_or(true))
        .map(|device| RobotDevice {
            id: device.udid,
            name: device.name,
            platform: Platform::Ios,
            state: match device.state.as_str() {
                "Booted" => DeviceState::Available,
                "Shutdown" => DeviceState::Offline,
                _ => DeviceState::Unknown,
            },
        })
        .collect())
}

//! Decodes XCTest UI payloads and CoreDevice inventory.

use robot_core::{Bounds, DeviceState, Platform, RobotDevice, RobotError, UiElement};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct XCTestElement {
    node_id: String,
    automation_id: Option<String>,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    role: String,
    bounds: XCTestBounds,
    clickable: bool,
    enabled: bool,
    #[serde(default)]
    focused: Option<bool>,
    #[serde(default)]
    checked: Option<bool>,
    #[serde(default)]
    selected: Option<bool>,
}

#[derive(Deserialize)]
struct XCTestBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl XCTestElement {
    pub(super) fn into_ui_element(self) -> UiElement {
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
struct DeviceList {
    result: DeviceListResult,
}

#[derive(Deserialize)]
struct DeviceListResult {
    devices: Vec<CoreDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreDevice {
    identifier: String,
    device_properties: CoreDeviceProperties,
    #[serde(default)]
    connection_properties: Option<CoreConnectionProperties>,
}

#[derive(Deserialize)]
struct CoreDeviceProperties {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreConnectionProperties {
    #[serde(default)]
    local_hostnames: Vec<String>,
    #[serde(default)]
    potential_hostnames: Vec<String>,
    #[serde(default)]
    tunnel_state: Option<String>,
    #[serde(default)]
    pairing_state: Option<String>,
}

pub(super) fn parse_devicectl_devices(output: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let devices: DeviceList = serde_json::from_str(output).map_err(|error| {
        RobotError::CommandFailed(format!("invalid CoreDevice device JSON: {error}"))
    })?;
    Ok(devices
        .result
        .devices
        .into_iter()
        .filter_map(|device| {
            let connection = device.connection_properties?;
            let state = match (
                connection.pairing_state.as_deref(),
                connection.tunnel_state.as_deref(),
            ) {
                (Some("unpaired"), _) => DeviceState::Unauthorized,
                (_, Some("connected")) => DeviceState::Available,
                (_, Some("unavailable" | "disconnected")) => DeviceState::Offline,
                _ => DeviceState::Unknown,
            };
            let udid = connection
                .local_hostnames
                .into_iter()
                .chain(connection.potential_hostnames)
                .filter_map(|hostname| {
                    hostname
                        .strip_suffix(".coredevice.local")
                        .map(str::to_owned)
                })
                .find(|candidate| {
                    candidate != &device.identifier
                        && candidate
                            .chars()
                            .any(|character| character.is_ascii_digit())
                })?;
            Some(RobotDevice {
                id: udid,
                name: format!("{} ({})", device.device_properties.name, device.identifier),
                platform: Platform::Ios,
                state,
            })
        })
        .collect())
}

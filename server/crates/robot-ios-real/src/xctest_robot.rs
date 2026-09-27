use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use mobile_robot_core::{
    Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement, UiRunnerStatus,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

use crate::xctest_agent::XCTestAgentBridge;

const ACTION_TIMEOUT: Duration = Duration::from_secs(45);

pub struct IosXCTestRobot {
    xcrun_path: String,
    agent: XCTestAgentBridge,
}

impl IosXCTestRobot {
    pub fn new(xcrun_path: impl Into<String>, agent: XCTestAgentBridge) -> Self {
        Self {
            xcrun_path: xcrun_path.into(),
            agent,
        }
    }

    async fn xcrun(&self, args: &[&str]) -> Result<String, RobotError> {
        let output = Command::new(&self.xcrun_path)
            .args(args)
            .output()
            .await
            .map_err(|error| RobotError::CommandFailed(format!("could not run xcrun: {error}")))?;
        if output.status.success() {
            return String::from_utf8(output.stdout).map_err(|error| {
                RobotError::CommandFailed(format!("xcrun returned invalid UTF-8: {error}"))
            });
        }
        Err(RobotError::CommandFailed(format!(
            "xcrun {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim(),
        )))
    }

    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RobotError> {
        self.agent.dispatch(method, params, ACTION_TIMEOUT).await
    }

    async fn devicectl_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        // CoreDevice only guarantees machine-readable output through a JSON file.
        // The table's hostname is allowed to be a friendly name, which is not an
        // Xcode destination UDID.
        let output_path =
            std::env::temp_dir().join(format!("mobile-mcp-devicectl-{}.json", Uuid::new_v4()));
        let output_path_string = output_path.to_string_lossy().into_owned();
        let output = Command::new(&self.xcrun_path)
            .args([
                "devicectl",
                "list",
                "devices",
                "--json-output",
                &output_path_string,
            ])
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not run xcrun devicectl: {error}"))
            })?;
        if !output.status.success() {
            return Err(RobotError::CommandFailed(format!(
                "xcrun devicectl list devices failed: {}",
                String::from_utf8_lossy(&output.stderr).trim(),
            )));
        }
        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not read CoreDevice JSON output: {error}"))
            });
        let _ = tokio::fs::remove_file(&output_path).await;
        parse_devicectl_devices(&contents?)
    }
}

#[async_trait]
impl Robot for IosXCTestRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.devicectl_devices().await
    }

    async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
        let result = self.dispatch("screenshot", json!({})).await?;
        let encoded = result
            .get("pngBase64")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RobotError::CommandFailed("iOS XCTest agent returned no screenshot payload".into())
            })?;
        STANDARD.decode(encoded).map_err(|error| {
            RobotError::CommandFailed(format!(
                "iOS XCTest agent returned invalid screenshot: {error}"
            ))
        })
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        validate_app_id(app_id)?;
        self.xcrun(&[
            "devicectl",
            "device",
            "process",
            "launch",
            "--device",
            device_id,
            app_id,
        ])
        .await?;
        self.dispatch("launch", json!({ "bundleId": app_id }))
            .await
            .map(|_| ())
    }

    async fn open_url(&self, _: &str, url: &str, app_id: Option<&str>) -> Result<(), RobotError> {
        let parsed = Url::parse(url).map_err(|_| RobotError::InvalidInput("invalid URL".into()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        if let Some(app_id) = app_id {
            validate_app_id(app_id)?;
        }
        self.dispatch("openUrl", json!({ "url": url, "appId": app_id }))
            .await
            .map(|_| ())
    }

    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        if path.trim().is_empty() {
            return Err(RobotError::InvalidInput(
                "app path must not be empty".into(),
            ));
        }
        self.xcrun(&[
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            device_id,
            path,
        ])
        .await
        .map(|_| ())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        validate_app_id(app_id)?;
        self.xcrun(&[
            "devicectl",
            "device",
            "uninstall",
            "app",
            "--device",
            device_id,
            app_id,
        ])
        .await
        .map(|_| ())
    }

    async fn tap(&self, _: &str, point: Point) -> Result<(), RobotError> {
        valid_point(point)?;
        self.dispatch("tap", json!({ "x": point.x, "y": point.y }))
            .await
            .map(|_| ())
    }

    async fn type_text(&self, _: &str, text: &str) -> Result<(), RobotError> {
        if text.is_empty() {
            return Err(RobotError::InvalidInput("text must not be empty".into()));
        }
        self.dispatch("text", json!({ "text": text }))
            .await
            .map(|_| ())
    }

    async fn swipe(
        &self,
        _: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        valid_point(start)?;
        valid_point(end)?;
        self.dispatch(
            "swipe",
            json!({
                "fromX": start.x, "fromY": start.y,
                "toX": end.x, "toY": end.y,
                "durationMs": duration_ms.unwrap_or(500),
            }),
        )
        .await
        .map(|_| ())
    }

    async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
        let result = self.dispatch("dump", json!({})).await?;
        let elements = result.get("elements").cloned().ok_or_else(|| {
            RobotError::CommandFailed("iOS XCTest agent returned no element list".into())
        })?;
        let elements: Vec<XCTestElement> = serde_json::from_value(elements).map_err(|error| {
            RobotError::CommandFailed(format!("invalid iOS XCTest element payload: {error}"))
        })?;
        Ok(elements
            .into_iter()
            .map(XCTestElement::into_ui_element)
            .collect())
    }

    async fn ui_runner_status(&self, _: &str) -> Result<UiRunnerStatus, RobotError> {
        match self
            .agent
            .dispatch("health", json!({}), Duration::from_secs(2))
            .await
        {
            Ok(_) => Ok(UiRunnerStatus {
                ready: true,
                detail: "XCTest agent connected".into(),
            }),
            Err(error) => Ok(UiRunnerStatus {
                ready: false,
                detail: error.to_string(),
            }),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct XCTestElement {
    node_id: String,
    automation_id: Option<String>,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    role: String,
    bounds: XCTestBounds,
    clickable: bool,
    enabled: bool,
}

#[derive(Deserialize)]
struct XCTestBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl XCTestElement {
    fn into_ui_element(self) -> UiElement {
        UiElement {
            class_name: self.role,
            resource_id: self.automation_id.or(Some(self.node_id)),
            text: self.text.or(self.value),
            content_description: self.label,
            bounds: Bounds {
                left: self.bounds.x.round() as i32,
                top: self.bounds.y.round() as i32,
                right: (self.bounds.x + self.bounds.width).round() as i32,
                bottom: (self.bounds.y + self.bounds.height).round() as i32,
            },
            clickable: self.clickable,
            enabled: self.enabled,
        }
    }
}

fn validate_device_id(device_id: &str) -> Result<(), RobotError> {
    if device_id.is_empty()
        || !device_id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-')
    {
        return Err(RobotError::InvalidInput("invalid iOS device id".into()));
    }
    Ok(())
}

fn validate_app_id(app_id: &str) -> Result<(), RobotError> {
    if app_id.is_empty()
        || !app_id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '-' | '_'))
    {
        return Err(RobotError::InvalidInput("invalid iOS app id".into()));
    }
    Ok(())
}

fn valid_point(point: Point) -> Result<(), RobotError> {
    if point.x.is_finite() && point.y.is_finite() && point.x >= 0.0 && point.y >= 0.0 {
        Ok(())
    } else {
        Err(RobotError::InvalidInput(
            "screen coordinates must be finite and non-negative".into(),
        ))
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
}

fn parse_devicectl_devices(output: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let devices: DeviceList = serde_json::from_str(output).map_err(|error| {
        RobotError::CommandFailed(format!("invalid CoreDevice device JSON: {error}"))
    })?;
    Ok(devices
        .result
        .devices
        .into_iter()
        .filter_map(|device| {
            let connection = device.connection_properties?;
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
                state: DeviceState::Available,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_coredevice_json_to_xcode_udids() {
        let devices = parse_devicectl_devices(r#"{"result":{"devices":[{"identifier":"F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6","deviceProperties":{"name":"ho-doan"},"connectionProperties":{"potentialHostnames":["00008130-000959E43EDA001C.coredevice.local","F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6.coredevice.local"]}}]}}"#).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "00008130-000959E43EDA001C");
        assert_eq!(devices[0].platform, Platform::Ios);
    }
}

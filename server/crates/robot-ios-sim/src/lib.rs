//! iOS Simulator driver backed by Xcode's `xcrun simctl`.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use serde::Deserialize;
use tokio::process::Command;

static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct IosSimulatorRobot {
    xcrun_path: String,
    artifact_dir: PathBuf,
}

impl IosSimulatorRobot {
    pub fn new(xcrun_path: impl Into<String>, artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            xcrun_path: xcrun_path.into(),
            artifact_dir: artifact_dir.into(),
        }
    }

    async fn simctl(&self, args: &[String]) -> Result<Vec<u8>, RobotError> {
        let output = Command::new(&self.xcrun_path)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start xcrun: {error}"))
            })?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(RobotError::CommandFailed(format!(
            "simctl exited with status {}: {}",
            output
                .status
                .code()
                .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    fn valid_id(value: &str, label: &str) -> Result<(), RobotError> {
        if value.trim().is_empty() {
            Err(RobotError::InvalidInput(format!(
                "{label} must not be empty"
            )))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Robot for IosSimulatorRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let stdout = self
            .simctl(&[
                "simctl".into(),
                "list".into(),
                "devices".into(),
                "available".into(),
                "-j".into(),
            ])
            .await?;
        parse_simctl_devices(&String::from_utf8(stdout).map_err(|error| {
            RobotError::CommandFailed(format!("simctl returned invalid UTF-8: {error}"))
        })?)
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        Self::valid_id(device_id, "device id")?;
        if self
            .list_devices()
            .await?
            .iter()
            .any(|device| device.id == device_id && device.state == DeviceState::Available)
        {
            return Ok(());
        }
        self.simctl(&["simctl".into(), "boot".into(), device_id.into()])
            .await?;
        self.simctl(&[
            "simctl".into(),
            "bootstatus".into(),
            device_id.into(),
            "-b".into(),
        ])
        .await?;
        Ok(())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        Self::valid_id(device_id, "device id")?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create screenshot directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "mobile-mcp-ios-{}-{}.png",
            std::process::id(),
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        self.simctl(&[
            "simctl".into(),
            "io".into(),
            device_id.into(),
            "screenshot".into(),
            path.to_string_lossy().into_owned(),
        ])
        .await?;
        let bytes = tokio::fs::read(&path).await.map_err(|error| {
            RobotError::CommandFailed(format!("could not read simulator screenshot: {error}"))
        })?;
        let _ = tokio::fs::remove_file(path).await;
        Ok(bytes)
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        Self::valid_id(device_id, "device id")?;
        Self::valid_id(app_id, "app id")?;
        self.simctl(&[
            "simctl".into(),
            "launch".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await
        .map(|_| ())
    }
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        _app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        Self::valid_id(device_id, "device id")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        self.simctl(&[
            "simctl".into(),
            "openurl".into(),
            device_id.into(),
            url.into(),
        ])
        .await
        .map(|_| ())
    }
    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        Self::valid_id(device_id, "device id")?;
        self.simctl(&[
            "simctl".into(),
            "install".into(),
            device_id.into(),
            path.into(),
        ])
        .await
        .map(|_| ())
    }
    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        Self::valid_id(device_id, "device id")?;
        self.simctl(&[
            "simctl".into(),
            "uninstall".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await
        .map(|_| ())
    }
    async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "iOS Simulator gestures require the Phase 2 XCTest runner".into(),
        ))
    }
    async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "iOS Simulator text input requires the Phase 2 XCTest runner".into(),
        ))
    }
    async fn swipe(&self, _: &str, _: Point, _: Point, _: Option<u64>) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "iOS Simulator gestures require the Phase 2 XCTest runner".into(),
        ))
    }
    async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
        Err(RobotError::Unsupported(
            "iOS Simulator accessibility requires the Phase 2 XCTest runner".into(),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_booted_and_shutdown_simulators() {
        let devices = parse_simctl_devices(r#"{"devices":{"runtime":[{"udid":"booted-id","name":"iPhone 16","state":"Booted","isAvailable":true},{"udid":"off-id","name":"iPhone SE","state":"Shutdown","isAvailable":true}]}}"#).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].state, DeviceState::Available);
        assert_eq!(devices[1].state, DeviceState::Offline);
    }
}

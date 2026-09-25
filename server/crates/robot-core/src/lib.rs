//! Platform-neutral contracts for mobile device drivers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RobotError {
    #[error("device driver command failed: {0}")]
    CommandFailed(String),
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RobotDevice {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub state: DeviceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Available,
    Offline,
    Unauthorized,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Async, platform-independent device control boundary.
#[async_trait]
pub trait Robot: Send + Sync {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError>;
    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError>;
    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError>;
    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError>;
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError>;
    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError>;
}

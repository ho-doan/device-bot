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

/// A rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// A normalized accessibility node from the platform UI tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiElement {
    pub class_name: String,
    pub resource_id: Option<String>,
    pub text: Option<String>,
    pub content_description: Option<String>,
    pub bounds: Bounds,
    pub clickable: bool,
    pub enabled: bool,
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
    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError>;
    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError>;
    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError>;
    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError>;
}

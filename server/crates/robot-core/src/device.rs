//! Device identity, desktop targets, and screen geometry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RobotDevice {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub state: DeviceState,
}

/// Desktop coordinates are global: secondary displays may have negative origins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDisplay {
    pub id: String,
    pub name: String,
    pub is_primary: bool,
    pub bounds: Bounds,
}

/// A process currently showing a window on a desktop host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    pub process_id: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
}

/// One window belonging to a running desktop process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppWindow {
    pub window_id: String,
    pub process_id: u32,
    pub title: String,
    #[serde(default)]
    pub focused: bool,
}

/// Request-scoped accessibility target. Omitted fields keep the previous selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiTarget {
    pub process_id: Option<u32>,
    pub window_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScreenshotOptions {
    pub app_id: Option<String>,
    pub display_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Chrome,
    Ios,
    Macos,
    Windows,
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

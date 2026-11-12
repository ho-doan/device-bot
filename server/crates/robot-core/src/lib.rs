//! Platform-neutral contracts for mobile device drivers.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RobotError {
    #[error(
        "device_locked: iPhone {0} is locked. Unlock the iPhone on the device, then retry the operation."
    )]
    DeviceLocked(String),
    #[error("device driver command failed: {0}")]
    CommandFailed(String),
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub mod health;

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

/// Normalized control kinds shared by MCP queries and driver adapters.
///
/// `textField` includes password, search, and multiline editors. Drivers that
/// cannot classify a node leave `UiElement::element_type` unset instead of
/// guessing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ElementType {
    TextField,
    Button,
    Link,
    StaticText,
    CheckBox,
    Switch,
    Image,
    Radio,
    Slider,
}

impl ElementType {
    pub const CONTRACT_NAMES: &'static str =
        "textField, button, link, staticText, checkBox, switch, image, radio, slider";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextField => "textField",
            Self::Button => "button",
            Self::Link => "link",
            Self::StaticText => "staticText",
            Self::CheckBox => "checkBox",
            Self::Switch => "switch",
            Self::Image => "image",
            Self::Radio => "radio",
            Self::Slider => "slider",
        }
    }

    pub fn parse(value: &str) -> Result<Self, RobotError> {
        match value {
            "textField" => Ok(Self::TextField),
            "button" => Ok(Self::Button),
            "link" => Ok(Self::Link),
            "staticText" => Ok(Self::StaticText),
            "checkBox" => Ok(Self::CheckBox),
            "switch" => Ok(Self::Switch),
            "image" => Ok(Self::Image),
            "radio" => Ok(Self::Radio),
            "slider" => Ok(Self::Slider),
            other => Err(RobotError::InvalidInput(format!(
                "unsupported elementType {other:?}; expected one of: {}",
                Self::CONTRACT_NAMES
            ))),
        }
    }
}

/// IME action on the focused editor. These are distinct actions; Enter is not
/// a substitute for a named action the field is not configured to perform.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImeAction {
    Next,
    Done,
    Go,
    Search,
    Send,
}

impl ImeAction {
    pub const CONTRACT_NAMES: &'static str = "next, done, go, search, send";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Done => "done",
            Self::Go => "go",
            Self::Search => "search",
            Self::Send => "send",
        }
    }

    /// Keyboard label XCTest should tap. This is an exact key, not a fallback list.
    pub fn keyboard_label(self) -> &'static str {
        match self {
            Self::Next => "Next",
            Self::Done => "Done",
            Self::Go => "Go",
            Self::Search => "Search",
            Self::Send => "Send",
        }
    }

    pub fn parse(value: &str) -> Result<Self, RobotError> {
        match value {
            "next" => Ok(Self::Next),
            "done" => Ok(Self::Done),
            "go" => Ok(Self::Go),
            "search" => Ok(Self::Search),
            "send" => Ok(Self::Send),
            other => Err(RobotError::InvalidInput(format!(
                "unsupported IME action {other:?}; expected one of: {}",
                Self::CONTRACT_NAMES
            ))),
        }
    }

    pub fn from_android_action_id(action_id: u32) -> Option<Self> {
        match action_id {
            2 => Some(Self::Go),
            3 => Some(Self::Search),
            4 => Some(Self::Send),
            5 => Some(Self::Next),
            6 => Some(Self::Done),
            _ => None,
        }
    }
}

/// Owned query so drivers can apply type and text before `max_results`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementQuery {
    pub filter: Option<String>,
    pub element_type: Option<ElementType>,
    pub max_results: Option<usize>,
}

/// A normalized accessibility node from the platform UI tree.
///
/// `focused`, `checked`, and `selected` are omitted from JSON when the driver
/// could not observe them. Absence means unknown, not false.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    /// Native editor value when the platform exposes one separately from the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_type: Option<ElementType>,
}

/// A UI input operation that can be executed in one driver session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiAction {
    Tap {
        x: f64,
        y: f64,
    },
    TapElement {
        filter: String,
    },
    Type {
        text: String,
    },
    Swipe {
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        duration_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiRunnerStatus {
    pub ready: bool,
    pub detail: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub expected_version: Option<String>,
    #[serde(default)]
    pub agent_version: Option<String>,
    #[serde(default)]
    pub cache_ready: bool,
    #[serde(default)]
    pub target_app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Value>,
}

/// One complete Annex B H.264 access unit, using the encoder presentation clock.
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub pts_us: u64,
    pub keyframe: bool,
}

pub struct VideoStream {
    pub width: u32,
    pub height: u32,
    pub frames: tokio::sync::mpsc::Receiver<Result<VideoFrame, RobotError>>,
}

/// Browser-native WebRTC media; Rust owns signaling and cancellation.
pub struct NativeVideoStream {
    pub sdp: String,
    pub width: u32,
    pub height: u32,
    pub stop: tokio::sync::watch::Sender<bool>,
    pub done: tokio::task::JoinHandle<()>,
}

/// Async, platform-independent device control boundary.
#[async_trait]
pub trait Robot: Send + Sync {
    async fn stream_context(&self, _id: &str) -> Result<Value, RobotError> {
        Ok(Value::Null)
    }
    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        self.swipe(
            id,
            at,
            Point {
                x: at.x - dx,
                y: at.y - dy,
            },
            Some(200),
        )
        .await
    }

    async fn negotiate_native_video(
        &self,
        _id: &str,
        _offer: &str,
    ) -> Result<Option<NativeVideoStream>, RobotError> {
        Ok(None)
    }
    async fn restart_device(&self, id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!("restart for {id}")))
    }

    /// Encoded video only. Dropping the receiver stops capture and releases resources.
    async fn start_video_stream(&self, device_id: &str) -> Result<VideoStream, RobotError> {
        Err(RobotError::Unsupported(format!(
            "realtime video for {device_id}"
        )))
    }
    /// Isolate per-client state while sharing physical drivers where appropriate.
    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        None
    }
    async fn close_session(&self) -> Result<(), RobotError> {
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError>;
    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        Err(RobotError::Unsupported(format!(
            "listing installed apps is not supported for device {device_id}"
        )))
    }
    async fn list_running_apps(&self, device_id: &str) -> Result<Vec<RunningApp>, RobotError> {
        Err(RobotError::Unsupported(format!(
            "listing running apps is not supported for device {device_id}"
        )))
    }
    async fn list_app_windows(
        &self,
        device_id: &str,
        _process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        Err(RobotError::Unsupported(format!(
            "listing windows is not supported for device {device_id}"
        )))
    }
    /// Remember the process/window used by later element queries in this session.
    ///
    /// Listing elements does not require bringing that target to the foreground.
    async fn select_ui_target(&self, device_id: &str, _target: UiTarget) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "UI target selection is not supported for device {device_id}"
        )))
    }
    /// Bring the selected process/window to the foreground. Separate from Get elements.
    async fn activate_ui_target(
        &self,
        device_id: &str,
        target: UiTarget,
    ) -> Result<(), RobotError> {
        self.select_ui_target(device_id, target).await
    }
    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "boot is not supported for device {device_id}"
        )))
    }
    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Err(RobotError::Unsupported(format!(
            "screen recording is not supported for device {device_id}"
        )))
    }
    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Err(RobotError::Unsupported(format!(
            "screen recording is not supported for device {device_id}"
        )))
    }
    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError>;
    async fn list_displays(&self, device_id: &str) -> Result<Vec<DesktopDisplay>, RobotError> {
        Err(RobotError::Unsupported(format!(
            "display selection is not supported for device {device_id}"
        )))
    }
    async fn take_screenshot_with_options(
        &self,
        device_id: &str,
        options: &ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        if options.app_id.is_some() || options.display_id.is_some() {
            return Err(RobotError::Unsupported(
                "app/display screenshot options are supported only on desktop devices".into(),
            ));
        }
        self.take_screenshot(device_id).await
    }
    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError>;
    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError>;
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError>;
    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError>;
    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "clearing text for {device_id}"
        )))
    }
    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError>;
    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError>;
    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError>;
    async fn execute_ui_actions(
        &self,
        device_id: &str,
        actions: &[UiAction],
    ) -> Result<(), RobotError> {
        for action in actions {
            match action {
                UiAction::Tap { x, y } => self.tap(device_id, Point { x: *x, y: *y }).await?,
                UiAction::TapElement { filter } => {
                    return Err(RobotError::Unsupported(format!(
                        "tap element is not supported for device {device_id}: {filter}"
                    )));
                }
                UiAction::Type { text } => self.type_text(device_id, text).await?,
                UiAction::Swipe {
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    duration_ms,
                } => {
                    self.swipe(
                        device_id,
                        Point {
                            x: *start_x,
                            y: *start_y,
                        },
                        Point {
                            x: *end_x,
                            y: *end_y,
                        },
                        *duration_ms,
                    )
                    .await?
                }
            }
        }
        Ok(())
    }
    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        Err(RobotError::Unsupported(format!(
            "UI runner status is not supported for device {device_id}"
        )))
    }
    /// Read-only readiness check; never builds, installs, or starts a helper.
    async fn check_device_health(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        self.ui_runner_status(device_id).await
    }
    /// Reuse the current helper, building/installing only when required.
    async fn prepare_helper(
        &self,
        device_id: &str,
        app_id: Option<&str>,
    ) -> Result<UiRunnerStatus, RobotError> {
        self.prepare_ui_runner(device_id, app_id, false).await
    }
    /// Restart/reinstall the helper from the versioned cache; never reboot the OS.
    async fn reboot_helper(
        &self,
        device_id: &str,
        app_id: Option<&str>,
    ) -> Result<UiRunnerStatus, RobotError> {
        self.prepare_ui_runner(device_id, app_id, true).await
    }
    async fn prepare_ui_runner(
        &self,
        device_id: &str,
        _app_id: Option<&str>,
        _reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        self.ui_runner_status(device_id).await
    }
    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError>;
    /// Optional Flutter discovery. Native queries remain independent, so a
    /// successful or ambiguous native result never causes a bridge request.
    async fn list_flutter_elements(&self, _device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(vec![])
    }
    /// Lists elements whose visible accessibility fields contain `filter`.
    ///
    /// Drivers with native query support should override this so the platform can
    /// avoid materializing an entire UI tree. The default preserves support for
    /// drivers that can only return their complete tree.
    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        let elements = self.list_elements(device_id).await?;
        Ok(apply_element_query(
            elements,
            &ElementQuery {
                filter: filter.map(str::to_owned),
                element_type: None,
                max_results,
            },
        ))
    }
    /// Applies `element_type` and `filter` before `max_results`.
    ///
    /// Queries without a type keep the driver's `list_elements_filtered` path.
    /// A typed query loads the tree and filters before truncating. Drivers that
    /// cap `list_elements` must override this so the cap is not applied first.
    async fn list_elements_query(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        if query.element_type.is_none() {
            return self
                .list_elements_filtered(device_id, query.filter.as_deref(), query.max_results)
                .await;
        }
        let elements = self.list_elements(device_id).await?;
        Ok(apply_element_query(elements, &query))
    }
    /// Presses one named IME action on the focused editor.
    ///
    /// The default is an explicit capability error. Sending Enter or moving focus
    /// is not a fallback for drivers that cannot perform the requested action.
    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "IME action {} is not supported for device {device_id}",
            action.as_str()
        )))
    }
}

/// Fills `element_type` from the native class or role when the adapter did not.
pub fn prepare_elements(elements: &mut [UiElement]) {
    for element in elements {
        if element.element_type.is_none() {
            element.element_type = classify_class_name(&element.class_name);
        }
    }
}

pub fn classify_class_name(class_name: &str) -> Option<ElementType> {
    let simple = class_name.rsplit(['.', '/']).next().unwrap_or(class_name);
    let normalized = simple.to_ascii_lowercase().replace('_', "");
    if normalized.contains("edittext")
        || normalized.contains("autocomplete")
        || matches!(
            normalized.as_str(),
            "textfield"
                | "securetextfield"
                | "textarea"
                | "searchfield"
                | "axtextfield"
                | "axtextarea"
                | "axsearchfield"
        )
    {
        return Some(ElementType::TextField);
    }
    if normalized.contains("checkbox") || normalized.contains("checkedtext") {
        return Some(ElementType::CheckBox);
    }
    if normalized.contains("radiobutton")
        || matches!(normalized.as_str(), "radio" | "axradiobutton")
    {
        return Some(ElementType::Radio);
    }
    if normalized.contains("switch") || normalized.contains("toggle") {
        return Some(ElementType::Switch);
    }
    if normalized.contains("seekbar") || matches!(normalized.as_str(), "slider" | "axslider") {
        return Some(ElementType::Slider);
    }
    if normalized.contains("button") {
        return Some(ElementType::Button);
    }
    if normalized.contains("image") || normalized == "img" {
        return Some(ElementType::Image);
    }
    if matches!(normalized.as_str(), "link" | "a" | "axlink") {
        return Some(ElementType::Link);
    }
    if matches!(
        normalized.as_str(),
        "text" | "statictext" | "textview" | "axstatictext"
    ) {
        return Some(ElementType::StaticText);
    }
    None
}

pub fn element_matches_filter(element: &UiElement, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    [
        Some(element.class_name.as_str()),
        element.resource_id.as_deref(),
        element.text.as_deref(),
        element.content_description.as_deref(),
        element.value.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(&filter))
}

/// Filters by type and text, then applies `max_results`.
pub fn apply_element_query(mut elements: Vec<UiElement>, query: &ElementQuery) -> Vec<UiElement> {
    prepare_elements(&mut elements);
    if let Some(filter) = query.filter.as_deref() {
        elements.retain(|element| element_matches_filter(element, filter));
    }
    if let Some(element_type) = query.element_type {
        elements.retain(|element| element.element_type == Some(element_type));
    }
    if let Some(max_results) = query.max_results {
        elements.truncate(max_results);
    }
    elements
}

/// Routes a device operation to the driver which currently owns that ID.
#[derive(Default)]
pub struct RobotRegistry {
    drivers: Vec<Arc<dyn Robot>>,
}

impl RobotRegistry {
    pub fn new(drivers: impl IntoIterator<Item = Arc<dyn Robot>>) -> Self {
        Self {
            drivers: drivers.into_iter().collect(),
        }
    }

    async fn driver_for(&self, device_id: &str) -> Result<Arc<dyn Robot>, RobotError> {
        for driver in &self.drivers {
            let Ok(devices) = driver.list_devices().await else {
                continue;
            };
            if devices.iter().any(|device| {
                device.id == device_id
                    || (device_id == "chrome:browser" && device.platform == Platform::Chrome)
            }) {
                return Ok(Arc::clone(driver));
            }
        }
        Err(RobotError::DeviceNotFound(device_id.into()))
    }
}

#[async_trait]
impl Robot for RobotRegistry {
    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        self.driver_for(id).await?.stream_context(id).await
    }
    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        self.driver_for(id).await?.scroll(id, at, dx, dy).await
    }

    async fn negotiate_native_video(
        &self,
        id: &str,
        offer: &str,
    ) -> Result<Option<NativeVideoStream>, RobotError> {
        self.driver_for(id)
            .await?
            .negotiate_native_video(id, offer)
            .await
    }
    async fn restart_device(&self, id: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.restart_device(id).await
    }

    async fn start_video_stream(&self, id: &str) -> Result<VideoStream, RobotError> {
        self.driver_for(id).await?.start_video_stream(id).await
    }
    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        Some(Arc::new(Self::new(
            self.drivers
                .iter()
                .map(|driver| driver.fork_session().unwrap_or_else(|| Arc::clone(driver)))
                .collect::<Vec<_>>(),
        )))
    }
    async fn close_session(&self) -> Result<(), RobotError> {
        let mut first = None;
        for driver in &self.drivers {
            if let Err(error) = driver.close_session().await {
                if first.is_none() {
                    first = Some(error);
                }
            }
        }
        first.map_or(Ok(()), Err)
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let mut devices = vec![];
        let mut errors = vec![];
        for driver in &self.drivers {
            match driver.list_devices().await {
                Ok(found) => devices.extend(found),
                Err(error) => errors.push(error),
            }
        }
        if devices.is_empty() && !errors.is_empty() {
            return Err(errors.remove(0));
        }
        Ok(devices)
    }
    async fn list_installed_apps(&self, id: &str) -> Result<Value, RobotError> {
        self.driver_for(id).await?.list_installed_apps(id).await
    }
    async fn list_running_apps(&self, id: &str) -> Result<Vec<RunningApp>, RobotError> {
        self.driver_for(id).await?.list_running_apps(id).await
    }
    async fn list_app_windows(
        &self,
        id: &str,
        process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        self.driver_for(id)
            .await?
            .list_app_windows(id, process_id)
            .await
    }
    async fn select_ui_target(&self, id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .select_ui_target(id, target)
            .await
    }
    async fn activate_ui_target(&self, id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .activate_ui_target(id, target)
            .await
    }
    async fn boot_device(&self, id: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.boot_device(id).await
    }
    async fn start_screen_recording(&self, id: &str) -> Result<String, RobotError> {
        self.driver_for(id).await?.start_screen_recording(id).await
    }
    async fn stop_screen_recording(&self, id: &str) -> Result<String, RobotError> {
        self.driver_for(id).await?.stop_screen_recording(id).await
    }
    async fn take_screenshot(&self, id: &str) -> Result<Vec<u8>, RobotError> {
        self.driver_for(id).await?.take_screenshot(id).await
    }
    async fn list_displays(&self, id: &str) -> Result<Vec<DesktopDisplay>, RobotError> {
        self.driver_for(id).await?.list_displays(id).await
    }
    async fn take_screenshot_with_options(
        &self,
        id: &str,
        options: &ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        self.driver_for(id)
            .await?
            .take_screenshot_with_options(id, options)
            .await
    }
    async fn tap(&self, id: &str, point: Point) -> Result<(), RobotError> {
        self.driver_for(id).await?.tap(id, point).await
    }
    async fn launch_app(&self, id: &str, app: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.launch_app(id, app).await
    }
    async fn open_url(&self, id: &str, url: &str, app: Option<&str>) -> Result<(), RobotError> {
        self.driver_for(id).await?.open_url(id, url, app).await
    }
    async fn clear_text(&self, id: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.clear_text(id).await
    }
    async fn type_text(&self, id: &str, text: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.type_text(id, text).await
    }
    async fn swipe(
        &self,
        id: &str,
        start: Point,
        end: Point,
        duration: Option<u64>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .swipe(id, start, end, duration)
            .await
    }
    async fn install_app(&self, id: &str, path: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.install_app(id, path).await
    }
    async fn uninstall_app(&self, id: &str, app: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.uninstall_app(id, app).await
    }
    async fn execute_ui_actions(&self, id: &str, actions: &[UiAction]) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .execute_ui_actions(id, actions)
            .await
    }
    async fn ui_runner_status(&self, id: &str) -> Result<UiRunnerStatus, RobotError> {
        self.driver_for(id).await?.ui_runner_status(id).await
    }
    async fn check_device_health(&self, id: &str) -> Result<UiRunnerStatus, RobotError> {
        self.driver_for(id).await?.check_device_health(id).await
    }
    async fn prepare_ui_runner(
        &self,
        id: &str,
        app_id: Option<&str>,
        reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        self.driver_for(id)
            .await?
            .prepare_ui_runner(id, app_id, reinstall)
            .await
    }
    async fn list_elements(&self, id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.driver_for(id).await?.list_elements(id).await
    }

    async fn list_flutter_elements(&self, id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.driver_for(id).await?.list_flutter_elements(id).await
    }

    async fn list_elements_filtered(
        &self,
        id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.driver_for(id)
            .await?
            .list_elements_filtered(id, filter, max_results)
            .await
    }

    async fn list_elements_query(
        &self,
        id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.driver_for(id)
            .await?
            .list_elements_query(id, query)
            .await
    }

    async fn press_ime_action(&self, id: &str, action: ImeAction) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .press_ime_action(id, action)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubRobot {
        device: Option<RobotDevice>,
        list_error: bool,
    }

    #[async_trait]
    impl Robot for StubRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            if self.list_error {
                return Err(RobotError::CommandFailed("driver unavailable".into()));
            }
            Ok(self.device.clone().into_iter().collect())
        }

        async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
            Ok(vec![7])
        }

        async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
            Ok(())
        }

        async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
            Ok(())
        }

        async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn swipe(
            &self,
            _: &str,
            _: Point,
            _: Point,
            _: Option<u64>,
        ) -> Result<(), RobotError> {
            Ok(())
        }

        async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
            Ok(vec![])
        }
    }

    fn element(class_name: &str, text: &str) -> UiElement {
        UiElement {
            class_name: class_name.into(),
            resource_id: None,
            text: Some(text.into()),
            content_description: None,
            bounds: Bounds {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            clickable: true,
            enabled: true,
            focused: None,
            checked: None,
            selected: None,
            value: None,
            element_type: None,
        }
    }

    struct TreeRobot {
        elements: Vec<UiElement>,
    }

    #[async_trait]
    impl Robot for TreeRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(vec![])
        }
        async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
            Ok(vec![])
        }
        async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
            Ok(())
        }
        async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
            Ok(())
        }
        async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn swipe(
            &self,
            _: &str,
            _: Point,
            _: Point,
            _: Option<u64>,
        ) -> Result<(), RobotError> {
            Ok(())
        }
        async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
            Ok(self.elements.clone())
        }
    }

    #[tokio::test]
    async fn element_type_and_text_filter_run_before_max_results() {
        let robot = TreeRobot {
            elements: vec![
                element("android.widget.Button", "Save"),
                element("android.widget.Button", "Cancel"),
                element("android.widget.EditText", "secret"),
                element("android.widget.EditText", "user@example.com"),
            ],
        };

        let first_text_field = robot
            .list_elements_query(
                "device",
                ElementQuery {
                    filter: None,
                    element_type: Some(ElementType::TextField),
                    max_results: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(first_text_field.len(), 1);
        assert_eq!(first_text_field[0].text.as_deref(), Some("secret"));
        assert_eq!(
            first_text_field[0].element_type,
            Some(ElementType::TextField)
        );

        let email = robot
            .list_elements_query(
                "device",
                ElementQuery {
                    filter: Some("user@example.com".into()),
                    element_type: Some(ElementType::TextField),
                    max_results: Some(2),
                },
            )
            .await
            .unwrap();
        assert_eq!(email.len(), 1);
        assert_eq!(email[0].text.as_deref(), Some("user@example.com"));

        let untyped = robot
            .list_elements_filtered("device", None, Some(1))
            .await
            .unwrap();
        assert_eq!(untyped[0].text.as_deref(), Some("Save"));
    }

    #[test]
    fn classifies_password_editors_as_text_fields_and_rejects_unknown_types() {
        assert_eq!(
            classify_class_name("android.widget.EditText"),
            Some(ElementType::TextField)
        );
        assert_eq!(
            classify_class_name("androidx.appcompat.widget.AppCompatEditText"),
            Some(ElementType::TextField)
        );
        assert_eq!(
            classify_class_name("android.widget.ImageButton"),
            Some(ElementType::Button)
        );
        assert!(ElementType::parse("EditText").is_err());
        assert!(
            ElementType::parse("not-a-type")
                .unwrap_err()
                .to_string()
                .contains("unsupported elementType")
        );
        assert_eq!(ImeAction::from_android_action_id(5), Some(ImeAction::Next));
        assert_eq!(
            ImeAction::from_android_action_id(0x06),
            Some(ImeAction::Done)
        );
        assert_eq!(ImeAction::from_android_action_id(1), None);
    }

    #[test]
    fn omitted_observation_fields_deserialize_as_unknown() {
        let element: UiElement = serde_json::from_str(
            r#"{"className":"android.widget.Button","text":null,"resourceId":null,"contentDescription":null,"bounds":{"left":0,"top":0,"right":1,"bottom":1},"clickable":true,"enabled":true}"#,
        )
        .unwrap();
        assert_eq!(element.focused, None);
        assert_eq!(element.checked, None);
        assert_eq!(element.selected, None);
        let json = serde_json::to_value(&element).unwrap();
        assert!(json.get("focused").is_none());
        assert!(json.get("checked").is_none());
    }

    #[tokio::test]
    async fn chrome_boot_alias_routes_to_driver_with_an_available_page() {
        let driver: Arc<dyn Robot> = Arc::new(StubRobot {
            device: Some(RobotDevice {
                id: "chrome:page".into(),
                name: "Page".into(),
                platform: Platform::Chrome,
                state: DeviceState::Available,
            }),
            list_error: false,
        });
        let registry = RobotRegistry::new(vec![Arc::clone(&driver)]);
        assert!(Arc::ptr_eq(
            &registry.driver_for("chrome:browser").await.unwrap(),
            &driver
        ));
    }

    #[tokio::test]
    async fn routes_to_a_healthy_driver_when_another_driver_is_unavailable() {
        let registry = RobotRegistry::new(vec![
            Arc::new(StubRobot {
                device: None,
                list_error: true,
            }) as Arc<dyn Robot>,
            Arc::new(StubRobot {
                device: Some(RobotDevice {
                    id: "ios-sim".into(),
                    name: "iPhone".into(),
                    platform: Platform::Ios,
                    state: DeviceState::Available,
                }),
                list_error: false,
            }),
        ]);

        assert_eq!(registry.list_devices().await.unwrap().len(), 1);
        assert_eq!(registry.take_screenshot("ios-sim").await.unwrap(), vec![7]);
    }
}

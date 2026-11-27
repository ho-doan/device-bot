//! Driver interface and helper readiness contracts.

use crate::{
    AppWindow, DesktopDisplay, ElementQuery, ImeAction, NativeVideoStream, Point, RobotDevice,
    RobotError, RunningApp, ScreenshotOptions, UiAction, UiElement, UiTarget, VideoStream,
    WebContext, WebViewport, apply_element_query,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

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

/// Async, platform-independent device control boundary.
#[async_trait]
pub trait Robot: Send + Sync {
    /// Read-only readiness. Unknown is never equivalent to available.
    async fn capabilities(&self, device_id: &str) -> Result<crate::CapabilityReport, RobotError> {
        Ok(crate::CapabilityReport::new(device_id))
    }

    async fn start_observation(
        &self,
        id: &str,
        _options: crate::ObservationOptions,
    ) -> Result<String, RobotError> {
        Err(RobotError::Unsupported(format!("observation for {id}")))
    }
    async fn poll_observation(
        &self,
        id: &str,
        _observation: &str,
        _after: u64,
        _limit: usize,
    ) -> Result<crate::ObservationBatch, RobotError> {
        Err(RobotError::Unsupported(format!("observation for {id}")))
    }
    async fn stop_observation(&self, id: &str, _observation: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!("observation for {id}")))
    }
    async fn scroll_in_context(
        &self,
        id: &str,
        point: Point,
        dx: f64,
        dy: f64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "context-bound DOM scrolling is unavailable".into(),
            ));
        }
        self.scroll(id, point, dx, dy).await
    }
    /// DOM activation must validate the node's document before acting.
    async fn tap_in_context(
        &self,
        id: &str,
        point: Point,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "context-bound DOM activation is unavailable".into(),
            ));
        }
        self.tap(id, point).await
    }
    /// Sends an explicit gesture, never silently downgrading to a single tap.
    async fn pointer_gesture(
        &self,
        id: &str,
        point: Point,
        gesture: crate::PointerGesture,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if gesture == crate::PointerGesture::Tap {
            return self.tap_in_context(id, point, context).await;
        }
        Err(RobotError::Unsupported(format!(
            "{} for {id}",
            gesture.name()
        )))
    }
    /// A press, move and release. With a DOM context the endpoints are fractions
    /// within the referenced element rather than absolute screen coordinates.
    async fn drag(
        &self,
        id: &str,
        start: Point,
        end: Point,
        duration: u64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "context-bound DOM dragging is unavailable".into(),
            ));
        }
        self.swipe(id, start, end, Some(duration)).await
    }
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
    /// Stable boot identity for a running virtual device, if the driver can
    /// verify it independently of the connection's transient serial.
    async fn recovery_boot_target(&self, _device_id: &str) -> Result<Option<String>, RobotError> {
        Ok(None)
    }
    async fn configure_web_proxy(
        &self,
        device_id: &str,
        _config: crate::WebProxyConfig,
    ) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "web proxy for {device_id}"
        )))
    }
    async fn set_web_fault(
        &self,
        device_id: &str,
        _fault: Option<crate::WebFault>,
    ) -> Result<crate::WebFaultTransition, RobotError> {
        Err(RobotError::Unsupported(format!(
            "web fault for {device_id}"
        )))
    }
    async fn web_fault_state(&self, device_id: &str) -> Result<crate::WebFaultState, RobotError> {
        Err(RobotError::Unsupported(format!(
            "web fault state for {device_id}"
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
    async fn terminate_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "app termination is not supported by this driver".into(),
        ))
    }
    async fn terminate_all_apps(&self, _device_id: &str) -> Result<Vec<String>, RobotError> {
        Err(RobotError::Unsupported(
            "app termination is not supported by this driver".into(),
        ))
    }
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError>;

    /// Open an entry URL in a fresh document, including for fragment-only navigation.
    async fn reset_web_document(&self, device_id: &str, url: &str) -> Result<(), RobotError> {
        self.open_url(device_id, url, None).await
    }

    /// Bind an explicitly opted-in desktop fixture to the selected window.
    async fn bind_window_fixture(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "Window fixture reset is unavailable".into(),
        ))
    }

    /// Relaunch the bound fixture and return its new, verified window-scoped ID.
    async fn restart_window_fixture(&self, _device_id: &str) -> Result<String, RobotError> {
        Err(RobotError::Unsupported(
            "Window fixture reset is unavailable".into(),
        ))
    }

    async fn web_viewport(&self, _device_id: &str) -> Result<WebViewport, RobotError> {
        Err(RobotError::Unsupported(
            "Browser viewport metrics unavailable".into(),
        ))
    }
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
                max_results,
                ..ElementQuery::default()
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
        if query
            .web_context
            .as_ref()
            .is_some_and(|context| !context.is_empty())
        {
            return Err(RobotError::Unsupported(format!(
                "web context queries are not supported for {device_id}"
            )));
        }
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
    /// Types into the focused editable in `context` when that document is not the unscoped root.
    async fn type_text_in_context(
        &self,
        device_id: &str,
        text: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if context.is_some_and(|context| !context.is_empty()) {
            return Err(RobotError::Unsupported(format!(
                "web context typing is not supported for {device_id}"
            )));
        }
        self.type_text(device_id, text).await
    }

    async fn clear_text_in_context(
        &self,
        device_id: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if context.is_some_and(|context| !context.is_empty()) {
            return Err(RobotError::Unsupported(format!(
                "web context clearing is not supported for {device_id}"
            )));
        }
        self.clear_text(device_id).await
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(format!(
            "IME action {} is not supported for device {device_id}",
            action.as_str()
        )))
    }
}

//! Session-aware routing from device identities to their owning drivers.

use crate::{
    AppWindow, DesktopDisplay, ElementQuery, ImeAction, NativeVideoStream, Platform, Point, Robot,
    RobotDevice, RobotError, RunningApp, ScreenshotOptions, UiAction, UiElement, UiRunnerStatus,
    UiTarget, VideoStream, WebContext, WebViewport,
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, OnceLock};

/// Routes a device operation to the driver which currently owns that ID.
#[derive(Default)]
pub struct RobotRegistry {
    drivers: Vec<Arc<dyn Robot>>,
    macos: OnceLock<Arc<dyn Robot>>,
}

impl RobotRegistry {
    pub fn new(drivers: impl IntoIterator<Item = Arc<dyn Robot>>) -> Self {
        Self {
            drivers: drivers.into_iter().collect(),
            macos: OnceLock::new(),
        }
    }

    async fn driver_for(&self, device_id: &str) -> Result<Arc<dyn Robot>, RobotError> {
        let macos_target = device_id == "macos-host" || device_id.starts_with("macos:");
        if macos_target && let Some(driver) = self.macos.get() {
            return Ok(Arc::clone(driver));
        }
        for driver in &self.drivers {
            let Ok(devices) = driver.list_devices().await else {
                continue;
            };
            if let Some(device) = devices.iter().find(|device| {
                device.id == device_id
                    || (device_id == "chrome:browser" && device.platform == Platform::Chrome)
                    // Window-scoped Windows IDs retain the selected PID/HWND for
                    // every operation. The driver validates the actual identity.
                    || (device_id.starts_with("windows:")
                        && device.id == "windows-host"
                        && device.platform == Platform::Windows)
                    || (device_id.starts_with("macos:")
                        && device.id == "macos-host"
                        && device.platform == Platform::Macos)
            }) {
                if macos_target && device.id == "macos-host" && device.platform == Platform::Macos {
                    // The local driver is fixed for this session. It still
                    // verifies each window identity, which can change at any time.
                    let _ = self.macos.set(Arc::clone(driver));
                }
                return Ok(Arc::clone(driver));
            }
        }
        Err(RobotError::DeviceNotFound(device_id.into()))
    }
}

#[async_trait]
impl Robot for RobotRegistry {
    async fn capabilities(&self, id: &str) -> Result<crate::CapabilityReport, RobotError> {
        self.driver_for(id).await?.capabilities(id).await
    }

    async fn start_observation(
        &self,
        id: &str,
        options: crate::ObservationOptions,
    ) -> Result<String, RobotError> {
        self.driver_for(id)
            .await?
            .start_observation(id, options)
            .await
    }
    async fn poll_observation(
        &self,
        id: &str,
        observation: &str,
        after: u64,
        limit: usize,
    ) -> Result<crate::ObservationBatch, RobotError> {
        self.driver_for(id)
            .await?
            .poll_observation(id, observation, after, limit)
            .await
    }
    async fn stop_observation(&self, id: &str, observation: &str) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .stop_observation(id, observation)
            .await
    }
    async fn scroll_in_context(
        &self,
        id: &str,
        point: Point,
        dx: f64,
        dy: f64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .scroll_in_context(id, point, dx, dy, context)
            .await
    }
    async fn type_text_in_context(
        &self,
        id: &str,
        text: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .type_text_in_context(id, text, context)
            .await
    }
    async fn clear_text_in_context(
        &self,
        id: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .clear_text_in_context(id, context)
            .await
    }
    async fn tap_in_context(
        &self,
        id: &str,
        point: Point,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .tap_in_context(id, point, context)
            .await
    }
    async fn pointer_gesture(
        &self,
        id: &str,
        point: Point,
        gesture: crate::PointerGesture,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .pointer_gesture(id, point, gesture, context)
            .await
    }
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
    async fn configure_web_proxy(
        &self,
        id: &str,
        config: crate::WebProxyConfig,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .configure_web_proxy(id, config)
            .await
    }
    async fn set_web_fault(
        &self,
        id: &str,
        fault: Option<crate::WebFault>,
    ) -> Result<crate::WebFaultTransition, RobotError> {
        self.driver_for(id).await?.set_web_fault(id, fault).await
    }
    async fn web_fault_state(&self, id: &str) -> Result<crate::WebFaultState, RobotError> {
        self.driver_for(id).await?.web_fault_state(id).await
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
    async fn terminate_app(&self, id: &str, app: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.terminate_app(id, app).await
    }
    async fn terminate_all_apps(&self, id: &str) -> Result<Vec<String>, RobotError> {
        self.driver_for(id).await?.terminate_all_apps(id).await
    }
    async fn open_url(&self, id: &str, url: &str, app: Option<&str>) -> Result<(), RobotError> {
        self.driver_for(id).await?.open_url(id, url, app).await
    }

    async fn reset_web_document(&self, id: &str, url: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.reset_web_document(id, url).await
    }

    async fn bind_window_fixture(&self, id: &str, app: &str) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .bind_window_fixture(id, app)
            .await
    }

    async fn restart_window_fixture(&self, id: &str) -> Result<String, RobotError> {
        self.driver_for(id).await?.restart_window_fixture(id).await
    }

    async fn web_viewport(&self, id: &str) -> Result<WebViewport, RobotError> {
        self.driver_for(id).await?.web_viewport(id).await
    }
    async fn clear_text(&self, id: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.clear_text(id).await
    }
    async fn type_text(&self, id: &str, text: &str) -> Result<(), RobotError> {
        self.driver_for(id).await?.type_text(id, text).await
    }
    async fn drag(
        &self,
        id: &str,
        start: Point,
        end: Point,
        duration: u64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.driver_for(id)
            .await?
            .drag(id, start, end, duration, context)
            .await
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

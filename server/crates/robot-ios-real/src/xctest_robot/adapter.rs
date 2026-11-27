//! Portable device operations backed by native platform modules.

use super::{
    ACTION_TIMEOUT, AGENT_PROBE_TIMEOUT, Arc, BRIDGE_UNREACHABLE, Command, Duration,
    IosXCTestRobot, PreparedLiveSource, QuickTimeRecording, SigningIssue, Url, Uuid, Value,
    XCTestElement, json, valid_point, validate_app_id, validate_device_id,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use robot_core::{
    DeviceState, ElementQuery, ImeAction, Point, Robot, RobotDevice, RobotError, UiAction,
    UiElement, UiRunnerStatus, WebContext, apply_element_query,
};

#[async_trait]
impl Robot for IosXCTestRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        let device = self
            .list_devices()
            .await?
            .into_iter()
            .find(|device| device.id == id)
            .ok_or_else(|| RobotError::DeviceNotFound(id.into()))?;
        let mut report = robot_core::CapabilityReport::new(id);
        report.capabilities.observation = robot_core::Capability::unsupported(
            "Native application log and network observation is not implemented",
        );
        report.capabilities.discovery =
            robot_core::Capability::available("Device was routed to the iOS driver");
        if device.state != DeviceState::Available {
            let blocked =
                robot_core::Capability::blocked("Connect, trust and unlock the iOS device");
            report.capabilities.launch = blocked.clone();
            report.capabilities.input = blocked.clone();
            report.capabilities.elements = blocked.clone();
            report.capabilities.screenshot = blocked.clone();
            report.capabilities.screen_recording = blocked;
            return Ok(report);
        }
        let runner = self.check_device_health(id).await;
        if runner.as_ref().is_ok_and(|status| {
            status.ready
                && status
                    .diagnostics
                    .as_ref()
                    .and_then(|d| d.get("screenLocked"))
                    .and_then(Value::as_bool)
                    == Some(false)
        }) {
            report.capabilities.launch = robot_core::Capability::available(
                "Connected unlocked device and current UI runner are responsive; launching an app can still fail",
            );
            let shot = self
                .agent
                .dispatch_for(id, "screenshot", json!({}), Duration::from_secs(8))
                .await;
            report.capabilities.screenshot = if shot
                .ok()
                .and_then(|v| {
                    v.get("pngBase64")
                        .and_then(Value::as_str)
                        .map(|s| s.starts_with("iVBORw0KGgo"))
                })
                .unwrap_or(false)
            {
                robot_core::Capability::available("The iOS UI runner returned a PNG screenshot")
            } else {
                robot_core::Capability::blocked(
                    "Check the iOS UI runner and device screenshot service",
                )
            };
        }
        let status = match runner {
            Ok(status)
                if status.ready
                    && status
                        .diagnostics
                        .as_ref()
                        .and_then(|d| d.get("screenLocked"))
                        .and_then(Value::as_bool)
                        != Some(false) =>
            {
                robot_core::Capability::unknown(
                    "The iPhone lock state could not be verified; unlock the device and check its connection",
                )
            }
            Ok(status) if status.ready => {
                let response = self
                    .agent
                    .dispatch_for(id, "dump", json!({"maxResults":1}), Duration::from_secs(5))
                    .await;
                if response
                    .ok()
                    .and_then(|value| value.get("elements").and_then(Value::as_array).cloned())
                    .is_some()
                {
                    robot_core::Capability::available(
                        "UI runner and target responded to a read-only element query; later actions can still fail",
                    )
                } else {
                    robot_core::Capability::blocked(
                        "Check the selected app and iOS UI runner; element query failed",
                    )
                }
            }
            _ => robot_core::Capability::blocked(
                "Prepare the iOS UI runner and select its target app",
            ),
        };
        report.capabilities.input = status.clone();
        report.capabilities.elements = status;
        Ok(report)
    }

    async fn scroll_in_context(
        &self,
        id: &str,
        point: Point,
        dx: f64,
        dy: f64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        let Some(context) = context.filter(|value| !value.is_empty()) else {
            return self.scroll(id, point, dx, dy).await;
        };
        self.webview_edit(id, context, "scroll", &json!({"dx":dx,"dy":dy}).to_string())
            .await
    }

    async fn tap_in_context(
        &self,
        id: &str,
        point: Point,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        let Some(context) = context.filter(|value| !value.is_empty()) else {
            return self.tap(id, point).await;
        };
        self.webview_edit(id, context, "tap", "").await
    }
    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        Some(Arc::new(Self {
            xcrun_path: self.xcrun_path.clone(),
            agent: self.agent.clone(),
            artifact_dir: self.artifact_dir.clone(),
            recording: self.recording.clone(),
            agent_launcher: self.agent_launcher.clone(),
            persistent_agents: self.persistent_agents.clone(),
            signing_issues: self.signing_issues.clone(),
            runner_project: self.runner_project.clone(),
            live_sources: Arc::default(),
        }))
    }

    async fn stream_context(&self, device_id: &str) -> Result<Value, RobotError> {
        validate_device_id(device_id)?;
        // A failed retry must not leave previously prepared capture geometry usable.
        self.live_sources.lock().await.remove(device_id);
        self.ensure_device_unlocked(device_id).await?;
        // Geometry is returned by XCTest in the same logical coordinates used
        // for tap/swipe. Video dimensions come from an actual USB video frame.
        let logical = self.dispatch(device_id, "screenInfo", json!({})).await?;
        let width = logical
            .get("width")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| {
                RobotError::Unsupported(
                    "Rebuild/reinstall the iOS XCTest runner for Live screenInfo support".into(),
                )
            })?;
        let height = logical
            .get("height")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| {
                RobotError::Unsupported(
                    "Rebuild/reinstall the iOS XCTest runner for Live screenInfo support".into(),
                )
            })?;
        let transport =
            std::env::var("DEVICE_BOT_IOS_LIVE_TRANSPORT").unwrap_or_else(|_| "auto".into());
        let usb = match transport.as_str() {
            "usb" => {
                Some(crate::live::prepare(&self.artifact_dir, &self.xcrun_path, device_id).await?)
            }
            "auto" => {
                crate::live::try_prepare(&self.artifact_dir, &self.xcrun_path, device_id).await?
            }
            "wifi" => None,
            _ => {
                return Err(RobotError::InvalidInput(
                    "DEVICE_BOT_IOS_LIVE_TRANSPORT must be auto, usb or wifi".into(),
                ));
            }
        };
        let (source, physical_width, physical_height, backend) = if let Some(source) = usb {
            crate::live::validate_geometry(width, height, source.width, source.height)?;
            let (w, h) = (source.width, source.height);
            (
                PreparedLiveSource::Usb(source),
                w,
                h,
                "avfoundation-usb-videotoolbox",
            )
        } else {
            let source = self.prepare_wifi_source(device_id, (width, height)).await?;
            let (w, h) = source.ready(Duration::from_secs(1)).await?;
            crate::live::validate_geometry(width, height, w, h)?;
            (
                PreparedLiveSource::Wifi(source),
                w,
                h,
                "replaykit-wifi-videotoolbox",
            )
        };
        let context = json!({"deviceId":device_id, "logicalWidth":width, "logicalHeight":height,
            "physicalWidth":physical_width, "physicalHeight":physical_height, "originX":0, "originY":0,
            "captureBackend":backend});
        self.live_sources
            .lock()
            .await
            .insert(device_id.into(), source);
        Ok(context)
    }

    async fn start_video_stream(
        &self,
        device_id: &str,
    ) -> Result<robot_core::VideoStream, RobotError> {
        validate_device_id(device_id)?;
        self.ensure_device_unlocked(device_id).await?;
        let source = self
            .live_sources
            .lock()
            .await
            .get(device_id)
            .cloned()
            .ok_or_else(|| {
                RobotError::InvalidInput("Prepare iOS stream context before starting Live".into())
            })?;
        match source {
            PreparedLiveSource::Usb(source) => crate::live::start(source).await,
            PreparedLiveSource::Wifi(source) => source.start().await,
        }
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.devicectl_devices().await
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        validate_device_id(device_id)?;
        let output_path =
            std::env::temp_dir().join(format!("mobile-mcp-devicectl-apps-{}.json", Uuid::new_v4()));
        let output_path_string = output_path.to_string_lossy().into_owned();
        let output = Command::new(&self.xcrun_path)
            .args([
                "devicectl",
                "device",
                "info",
                "apps",
                "--device",
                device_id,
                "--include-all-apps",
                "--json-output",
                &output_path_string,
            ])
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not run xcrun devicectl device info apps: {error}"
                ))
            })?;
        if !output.status.success() {
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(RobotError::CommandFailed(format!(
                "xcrun devicectl device info apps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not read devicectl apps JSON output: {error}"
                ))
            });
        let _ = tokio::fs::remove_file(&output_path).await;
        let contents = contents?;
        let apps = serde_json::from_str::<Value>(&contents).map_err(|error| {
            RobotError::CommandFailed(format!("invalid devicectl apps JSON output: {error}"))
        })?;
        Ok(apps)
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        validate_device_id(device_id)?;
        let result = self.dispatch(device_id, "screenshot", json!({})).await?;
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

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        validate_device_id(device_id)?;
        let mut recording = self.recording.lock().await;
        if recording.is_some() {
            return Err(RobotError::InvalidInput(format!(
                "an iOS real-device screen recording is already running for {device_id}"
            )));
        }
        let device_hint = self.quicktime_device_hint(device_id).await;
        let quicktime = QuickTimeRecording::start(&self.artifact_dir, &device_hint).await?;
        let path = quicktime.path();
        *recording = Some(quicktime);
        Ok(path)
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        validate_device_id(device_id)?;
        let Some(recording) = self.recording.lock().await.take() else {
            return Err(RobotError::InvalidInput(format!(
                "no iOS real-device screen recording is running for {device_id}"
            )));
        };
        recording.stop().await
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
        self.dispatch(device_id, "launch", json!({ "bundleId": app_id }))
            .await
            .map(|_| ())
    }

    async fn terminate_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        validate_app_id(app_id)?;
        self.dispatch(device_id, "terminate", json!({ "bundleId": app_id }))
            .await
            .map(|_| ())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        let parsed = Url::parse(url).map_err(|_| RobotError::InvalidInput("invalid URL".into()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        if let Some(app_id) = app_id {
            validate_app_id(app_id)?;
        }
        self.dispatch(device_id, "openUrl", json!({ "url": url, "appId": app_id }))
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

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        valid_point(point)?;
        self.dispatch(device_id, "tap", json!({ "x": point.x, "y": point.y }))
            .await
            .map(|_| ())
    }
    async fn pointer_gesture(
        &self,
        device_id: &str,
        point: Point,
        gesture: robot_core::PointerGesture,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if gesture == robot_core::PointerGesture::Tap {
            return self.tap_in_context(device_id, point, context).await;
        }
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "iOS WebView gestures require native screen coordinates".into(),
            ));
        }
        validate_device_id(device_id)?;
        valid_point(point)?;
        let mut params = json!({ "x": point.x, "y": point.y });
        if let robot_core::PointerGesture::LongPress { duration_ms } = gesture {
            params["durationMs"] = json!(duration_ms);
        }
        self.dispatch(device_id, gesture.name(), params)
            .await
            .map(|_| ())
    }

    async fn type_text_in_context(
        &self,
        device_id: &str,
        text: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        let Some(context) = context.filter(|context| !context.is_empty()) else {
            return self.type_text(device_id, text).await;
        };
        self.webview_edit(device_id, context, "type", text).await
    }

    async fn clear_text_in_context(
        &self,
        device_id: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        let Some(context) = context.filter(|context| !context.is_empty()) else {
            return self.clear_text(device_id).await;
        };
        self.webview_edit(device_id, context, "clear", "").await
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        if text.is_empty() || text.contains('\0') {
            return Err(RobotError::InvalidInput(
                "text must be non-empty and must not contain NUL".into(),
            ));
        }
        self.dispatch(device_id, "text", json!({ "text": text }))
            .await
            .map(|_| ())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        valid_point(start)?;
        valid_point(end)?;
        self.dispatch(
            device_id,
            "swipe",
            json!({
                "fromX": start.x, "fromY": start.y,
                "toX": end.x, "toY": end.y,
                "durationMs": duration_ms.unwrap_or(100),
            }),
        )
        .await
        .map(|_| ())
    }

    async fn execute_ui_actions(
        &self,
        device_id: &str,
        actions: &[UiAction],
    ) -> Result<(), RobotError> {
        if actions.is_empty() {
            return Err(RobotError::InvalidInput("actions must not be empty".into()));
        }
        validate_device_id(device_id)?;
        for action in actions {
            match action {
                UiAction::Tap { x, y } => self.tap(device_id, Point { x: *x, y: *y }).await?,
                UiAction::TapElement { filter } => {
                    self.tap_element_by_filter(device_id, filter).await?
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

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_filtered(device_id, None, None).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_query(
            device_id,
            ElementQuery {
                filter: filter.map(str::to_owned),
                max_results,
                ..Default::default()
            },
        )
        .await
    }

    async fn list_elements_query(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        query.reject_keyboard_exclusion()?;
        if let Some(context) = query
            .web_context
            .as_ref()
            .filter(|context| !context.is_empty())
        {
            return self.webview_elements(device_id, context, &query).await;
        }
        validate_device_id(device_id)?;
        let mut params = serde_json::Map::new();
        if let Some(filter) = query.filter.as_ref() {
            params.insert("filter".into(), Value::String(filter.clone()));
        }
        if let Some(element_type) = query.element_type {
            params.insert(
                "elementType".into(),
                Value::String(element_type.as_str().into()),
            );
        }
        if let Some(max_results) = query.max_results {
            params.insert("maxResults".into(), Value::from(max_results));
        }
        let result = self
            .dispatch(device_id, "dump", Value::Object(params))
            .await?;
        if let Some(element_type) = query.element_type
            && result.get("appliedElementType").and_then(Value::as_str)
                != Some(element_type.as_str())
        {
            return Err(RobotError::Unsupported(
                "iOS XCTest agent did not apply elementType; rebuild and reinstall the runner before querying by type".into(),
            ));
        }
        let elements = result.get("elements").cloned().ok_or_else(|| {
            RobotError::CommandFailed("iOS XCTest agent returned no element list".into())
        })?;
        let elements: Vec<XCTestElement> = serde_json::from_value(elements).map_err(|error| {
            RobotError::CommandFailed(format!("invalid iOS XCTest element payload: {error}"))
        })?;
        let elements = elements
            .into_iter()
            .map(XCTestElement::into_ui_element)
            .collect();
        Ok(apply_element_query(elements, &query))
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        match self
            .dispatch(
                device_id,
                "ime-action",
                json!({ "action": action.as_str() }),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(RobotError::CommandFailed(message)) if message.contains("unknown_method") => {
                Err(RobotError::Unsupported(
                    "iOS XCTest agent does not support IME actions; rebuild and reinstall the runner".into(),
                ))
            }
            Err(error) => Err(error),
        }
    }

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        // A foreground check guards an automation operation. Keep the
        // read-only command pending across a transient LAN poll failure
        // instead of restarting the helper on a two-second UI probe.
        self.agent_status(device_id, ACTION_TIMEOUT).await
    }
    async fn check_device_health(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        let (status, report, screen_locked) = tokio::join!(
            self.agent_status(device_id, AGENT_PROBE_TIMEOUT),
            self.connection_report(device_id, false),
            crate::connection::device_lock_state(&self.xcrun_path, device_id)
        );
        let mut status = status?;
        let report = report?;
        if !report.connected {
            status.ready = false;
            status.state = "offline".into();
            status.detail = report.summary();
        } else if screen_locked == Some(true) {
            status.ready = false;
            status.state = "device_locked".into();
            status.detail = "The iPhone screen is locked. Unlock the iPhone on the device, then retry the operation in QC.".into();
        }
        status.diagnostics = Some(json!(report));
        if let Some(diagnostics) = status.diagnostics.as_mut() {
            diagnostics["screenLocked"] = json!(screen_locked);
            if screen_locked == Some(true) {
                diagnostics["recommendedActions"] = json!(["unlock_device", "retry_operation"]);
            }
        }
        Ok(status)
    }

    async fn prepare_ui_runner(
        &self,
        device_id: &str,
        app_id: Option<&str>,
        reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        let status = self.check_device_health(device_id).await?;
        if status.state == "device_locked" {
            return Ok(status);
        }
        if !reinstall
            && status.ready
            && app_id.is_none_or(|app| status.target_app_id.as_deref() == Some(app))
        {
            return Ok(status);
        }
        let report = self.connection_report(device_id, true).await?;
        if !report.can_install() {
            return Err(RobotError::CommandFailed(report.summary()));
        }
        if self.agent_launcher.is_none() {
            return Err(RobotError::Unsupported("Real-iOS helper setup needs a signed runner and a reachable bridge. Configure DEVICE_BOT_IOS_AGENT_BIND, DEVICE_BOT_IOS_AGENT_HOSTS and the shared agent token on the Mac".into()));
        }
        let target = if app_id.is_some() {
            app_id.map(str::to_owned)
        } else {
            self.agent_status(device_id, AGENT_PROBE_TIMEOUT)
                .await
                .ok()
                .and_then(|s| s.target_app_id)
        };
        if reinstall {
            let _ = self
                .agent
                .dispatch_for(device_id, "stop", json!({}), AGENT_PROBE_TIMEOUT)
                .await;
            if let Some(mut child) = self.persistent_agents.lock().await.remove(device_id) {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        if let Err(error) = self.ensure_agent_started(device_id).await {
            let status = self.check_device_health(device_id).await?;
            if matches!(status.state.as_str(), "device_locked" | BRIDGE_UNREACHABLE)
                || SigningIssue::is_signing_state(&status.state)
                || (status.state == "trust_required"
                    && error.to_string().contains("iOS XCTest helper exited with"))
            {
                return Ok(status);
            }
            return Err(error);
        }
        if let Some(app_id) = target {
            self.launch_app(device_id, &app_id).await?;
        }
        self.agent_status(device_id, AGENT_PROBE_TIMEOUT).await
    }
}

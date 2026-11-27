//! Portable device operations backed by native platform modules.

use super::{
    ARTIFACT_SEQUENCE, ActiveRecording, Arc, Command, Duration, IosSimulatorRobot, Ordering,
    RunnerCache, Stdio, Value, decode_agent_elements, json, live_status, parse_simctl_devices,
};
use async_trait::async_trait;
use robot_core::{
    DeviceState, ElementQuery, ImeAction, Point, Robot, RobotDevice, RobotError, UiAction,
    UiElement, UiRunnerStatus, WebContext, apply_element_query,
};

#[async_trait]
impl Robot for IosSimulatorRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        self.capability_report(id).await
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
        let mut session = self.clone();
        session.live_targets = Arc::default();
        Some(Arc::new(session))
    }

    async fn stream_context(&self, device_id: &str) -> Result<Value, RobotError> {
        self.live_context(device_id).await
    }

    async fn start_video_stream(
        &self,
        device_id: &str,
    ) -> Result<robot_core::VideoStream, RobotError> {
        self.start_live_video(device_id).await
    }

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

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        Self::valid_device_id(device_id)?;
        let apps = self
            .simctl_json(&["simctl".into(), "listapps".into(), device_id.into()])
            .await?;
        Ok(json!({
            "platform": "ios",
            "deviceId": device_id,
            "apps": apps,
        }))
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        // bootstatus -b is idempotent and waits for readiness even when another
        // caller has already started the simulator but it is still booting.
        tokio::time::timeout(
            Duration::from_secs(180),
            self.simctl(&[
                "simctl".into(),
                "bootstatus".into(),
                device_id.into(),
                "-b".into(),
            ]),
        ).await.map_err(|_| RobotError::CommandFailed(format!(
            "iOS Simulator {device_id} did not finish starting within 180 seconds; refresh devices before retrying"
        )))??;
        Ok(())
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Self::valid_device_id(device_id)?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create recording directory: {error}"))
            })?;

        let mut recordings = self.recordings.lock().await;
        if recordings.contains_key(device_id) {
            return Err(RobotError::InvalidInput(format!(
                "a screen recording is already running for device {device_id}"
            )));
        }
        let preflight = self.check_recording_session(device_id).await?;
        if preflight.check.status != robot_core::health::CheckStatus::Pass {
            return Err(RobotError::CommandFailed(format!(
                "recording preflight blocked: {}. Run device-bot --health {device_id}",
                serde_json::to_string(&preflight)
                    .unwrap_or_else(|_| preflight.check.detail.clone())
            )));
        }
        let path = self.next_recording_path(device_id);
        let child = Command::new(&self.xcrun_path)
            .args([
                "simctl",
                "io",
                device_id,
                "recordVideo",
                &path.to_string_lossy(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start simctl screen recording: {error}"
                ))
            })?;
        recordings.insert(
            device_id.into(),
            ActiveRecording {
                child,
                path: path.clone(),
            },
        );
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Self::valid_device_id(device_id)?;
        let Some(mut recording) = self.recordings.lock().await.remove(device_id) else {
            return Err(RobotError::InvalidInput(format!(
                "no screen recording is running for device {device_id}"
            )));
        };
        #[cfg(unix)]
        if let Some(pid) = recording.child.id() {
            // `simctl io recordVideo` finalizes its MP4 when it receives SIGINT.
            unsafe { libc::kill(pid as i32, libc::SIGINT) };
        }
        #[cfg(not(unix))]
        recording.child.start_kill().map_err(|error| {
            RobotError::CommandFailed(format!("could not stop simctl screen recording: {error}"))
        })?;
        recording.child.wait().await.map_err(|error| {
            RobotError::CommandFailed(format!("could not stop simctl screen recording: {error}"))
        })?;
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        Self::valid_device_id(device_id)?;
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
        Self::valid_device_id(device_id)?;
        Self::valid_id(app_id, "app id")?;
        self.simctl(&[
            "simctl".into(),
            "launch".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await?;
        self.active_apps
            .lock()
            .await
            .insert(device_id.into(), app_id.into());
        Ok(())
    }

    async fn terminate_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        if !app_id
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            || !app_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(RobotError::InvalidInput(
                "invalid app bundle identifier".into(),
            ));
        }
        self.simctl(&[
            "simctl".into(),
            "terminate".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await?;
        let mut active = self.active_apps.lock().await;
        if active.get(device_id).is_some_and(|id| id == app_id) {
            active.remove(device_id);
        }
        Ok(())
    }
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
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
        .await?;
        self.active_apps.lock().await.insert(
            device_id.into(),
            app_id.unwrap_or("com.apple.mobilesafari").into(),
        );
        Ok(())
    }
    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
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
        Self::valid_device_id(device_id)?;
        self.simctl(&[
            "simctl".into(),
            "uninstall".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await?;
        let mut active_apps = self.active_apps.lock().await;
        if active_apps
            .get(device_id)
            .is_some_and(|active| active == app_id)
        {
            active_apps.remove(device_id);
        }
        Ok(())
    }
    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        Self::valid_point(point, "tap")?;
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "tap",
            &[
                ("DEVICE_BOT_X", point.x.to_string()),
                ("DEVICE_BOT_Y", point.y.to_string()),
            ],
            None,
        )
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
                "iOS Simulator WebView gestures require native screen coordinates".into(),
            ));
        }
        Self::valid_point(point, gesture.name())?;
        let app = self.active_app(device_id).await?;
        let mut params = vec![
            ("DEVICE_BOT_X", point.x.to_string()),
            ("DEVICE_BOT_Y", point.y.to_string()),
        ];
        if let robot_core::PointerGesture::LongPress { duration_ms } = gesture {
            params.push(("DEVICE_BOT_DURATION_MS", duration_ms.to_string()));
        }
        self.run_xctest(device_id, &app, gesture.name(), &params, None)
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
        if text.is_empty() {
            return Err(RobotError::InvalidInput("text must not be empty".into()));
        }
        if text.contains('\0') {
            return Err(RobotError::InvalidInput("text must not contain NUL".into()));
        }
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "type",
            &[("DEVICE_BOT_TEXT", text.into())],
            None,
        )
        .await
        .map(|_| ())
    }

    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(device_id, &app_id, "clear-text", &[], None)
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
        Self::valid_point(start, "swipe start")?;
        Self::valid_point(end, "swipe end")?;
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "swipe",
            &[
                ("DEVICE_BOT_X", start.x.to_string()),
                ("DEVICE_BOT_Y", start.y.to_string()),
                ("DEVICE_BOT_END_X", end.x.to_string()),
                ("DEVICE_BOT_END_Y", end.y.to_string()),
                (
                    "DEVICE_BOT_DURATION_SECONDS",
                    format!("{:.3}", duration_ms.unwrap_or(100) as f64 / 1_000.0),
                ),
            ],
            None,
        )
        .await
        .map(|_| ())
    }
    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_filtered(device_id, None, None).await
    }

    async fn execute_ui_actions(
        &self,
        device_id: &str,
        actions: &[UiAction],
    ) -> Result<(), RobotError> {
        if actions.is_empty() {
            return Err(RobotError::InvalidInput("actions must not be empty".into()));
        }
        let app_id = self.active_app(device_id).await?;
        for action in actions {
            match action {
                UiAction::Tap { x, y } => {
                    Self::valid_point(Point { x: *x, y: *y }, "tap")?;
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "tap",
                        &[
                            ("DEVICE_BOT_X", x.to_string()),
                            ("DEVICE_BOT_Y", y.to_string()),
                        ],
                        None,
                    )
                    .await?;
                }
                UiAction::TapElement { filter } => {
                    if filter.trim().is_empty() {
                        return Err(RobotError::InvalidInput(
                            "tap element filter must not be empty".into(),
                        ));
                    }
                    if filter.to_ascii_lowercase().contains("send") {
                        self.tap_element_by_filter(device_id, filter).await?;
                    } else {
                        let result = self
                            .run_xctest(
                                device_id,
                                &app_id,
                                "tap-element",
                                &[("DEVICE_BOT_ELEMENT_FILTER", filter.clone())],
                                None,
                            )
                            .await?;
                        if result.get("ok").and_then(Value::as_bool) == Some(false) {
                            return Err(RobotError::CommandFailed(format!(
                                "could not tap UI element matching {:?}",
                                filter
                            )));
                        }
                    }
                }
                UiAction::Type { text } => {
                    if text.is_empty() || text.contains('\0') {
                        return Err(RobotError::InvalidInput(
                            "text must be non-empty and must not contain NUL".into(),
                        ));
                    }
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "type",
                        &[("DEVICE_BOT_TEXT", text.clone())],
                        None,
                    )
                    .await?;
                }
                UiAction::Swipe {
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    duration_ms,
                } => {
                    Self::valid_point(
                        Point {
                            x: *start_x,
                            y: *start_y,
                        },
                        "swipe start",
                    )?;
                    Self::valid_point(
                        Point {
                            x: *end_x,
                            y: *end_y,
                        },
                        "swipe end",
                    )?;
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "swipe",
                        &[
                            ("DEVICE_BOT_X", start_x.to_string()),
                            ("DEVICE_BOT_Y", start_y.to_string()),
                            ("DEVICE_BOT_END_X", end_x.to_string()),
                            ("DEVICE_BOT_END_Y", end_y.to_string()),
                            (
                                "DEVICE_BOT_DURATION_SECONDS",
                                format!("{:.3}", duration_ms.unwrap_or(100) as f64 / 1_000.0),
                            ),
                        ],
                        None,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        Self::valid_device_id(device_id)?;
        let derived = Self::xctest_derived_data();
        let cache = RunnerCache {
            project: &self.xctest_project,
            derived_data: &derived,
            simulator: true,
        };
        let expected = cache.version()?;
        let health = match &self.simulator_agent {
            Some(agent) => {
                agent
                    .dispatch_for(device_id, "health", json!({}), Duration::from_secs(2))
                    .await
            }
            None => Err(RobotError::Unsupported(
                "Simulator helper bridge is not configured".into(),
            )),
        };
        let status = live_status(
            device_id,
            Some(expected.clone()),
            cache.is_current(&expected),
            health,
        );
        if status.ready {
            self.active_apps
                .lock()
                .await
                .insert(device_id.into(), status.target_app_id.clone().unwrap());
        }
        Ok(status)
    }

    async fn prepare_ui_runner(
        &self,
        device_id: &str,
        app_id: Option<&str>,
        reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        Self::valid_device_id(device_id)?;
        let _lock = self.xctest_lock.lock().await;
        let booted = self
            .list_devices()
            .await?
            .iter()
            .any(|d| d.id == device_id && d.state == DeviceState::Available);
        if !booted {
            return Err(RobotError::InvalidInput(
                "Boot the simulator before preparing its helper".into(),
            ));
        }
        if let Some(app_id) = app_id {
            Self::valid_id(app_id, "app id")?;
            self.simctl(&[
                "simctl".into(),
                "get_app_container".into(),
                device_id.into(),
                app_id.into(),
                "app".into(),
            ])
            .await?;
        }
        // Recover the target from a helper left alive by an earlier MCP process.
        let recovered_target = self
            .ui_runner_status(device_id)
            .await
            .ok()
            .filter(|status| status.state != "wrong_device")
            .and_then(|status| status.target_app_id);
        if reinstall {
            self.stop_persistent_agent(device_id).await;
        }
        let target = app_id
            .map(str::to_owned)
            .or(self.active_apps.lock().await.get(device_id).cloned())
            .or(recovered_target)
            .unwrap_or_default();
        let derived = Self::xctest_derived_data();
        self.ensure_persistent_agent(
            device_id,
            &target,
            &self.xctest_project.to_string_lossy(),
            &format!("platform=iOS Simulator,id={device_id}"),
            &derived,
        )
        .await?;
        self.ui_runner_status(device_id).await
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
        if let Some(context) = query
            .web_context
            .as_ref()
            .filter(|context| !context.is_empty())
        {
            query.reject_keyboard_exclusion()?;
            return self.webview_elements(device_id, context, &query).await;
        }
        let app_id = self.active_app(device_id).await?;
        let mut values = Vec::new();
        if let Some(element_type) = query.element_type {
            values.push(("DEVICE_BOT_ELEMENT_TYPE", element_type.as_str().to_owned()));
        }
        if query.exclude_system_keyboard {
            values.push(("DEVICE_BOT_EXCLUDE_KEYBOARD", "1".into()));
        }
        if let Some(filter) = query.filter.as_deref() {
            values.push(("DEVICE_BOT_ELEMENT_FILTER", filter.to_owned()));
        }
        if let Some(max_results) = query.max_results {
            values.push(("DEVICE_BOT_MAX_RESULTS", max_results.to_string()));
        }
        let result = self
            .run_xctest(device_id, &app_id, "list-elements", &values, None)
            .await?;
        if let Some(element_type) = query.element_type
            && result.get("appliedElementType").and_then(Value::as_str)
                != Some(element_type.as_str())
        {
            return Err(RobotError::Unsupported(
                "iOS Simulator XCTest agent did not apply elementType; rebuild and reinstall the runner before querying by type".into(),
            ));
        }
        if query.exclude_system_keyboard && result["excludedKeyboard"] != true {
            return Err(RobotError::Unsupported(
                "The simulator helper did not filter the system keyboard".into(),
            ));
        }
        let elements = decode_agent_elements(&result)?;
        Ok(apply_element_query(elements, &query))
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        let app_id = self.active_app(device_id).await?;
        match self
            .run_xctest(
                device_id,
                &app_id,
                "ime-action",
                &[("DEVICE_BOT_IME_ACTION", action.as_str().to_owned())],
                None,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(RobotError::CommandFailed(message)) if message.contains("unknown_method") => {
                Err(RobotError::Unsupported(
                    "iOS Simulator XCTest agent does not support IME actions; rebuild and reinstall the runner".into(),
                ))
            }
            Err(error) => Err(error),
        }
    }
}

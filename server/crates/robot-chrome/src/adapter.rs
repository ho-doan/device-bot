//! Portable device operations backed by native platform modules.

use super::{
    ActiveRecording, Arc, ChromeRobot, Duration, MissedTickBehavior, Ordering, RECORDING_SEQUENCE,
    Stdio, Value, connection, device_id, ffmpeg, interval, json, oneshot, test_proxy,
};
use async_trait::async_trait;
use robot_core::{
    DeviceState, ElementQuery, Platform, Point, Robot, RobotDevice, RobotError, UiElement,
    WebContext, apply_element_query,
};
use tokio::io::AsyncWriteExt;

#[async_trait]
impl Robot for ChromeRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        let mut report = robot_core::CapabilityReport::new(id);
        let target = self.page(id).await?;
        report.capabilities.discovery =
            robot_core::Capability::available("Chrome page is discoverable");
        let domains = self.cdp(&target, "Schema.getDomains", json!({})).await;
        let domains = domains
            .ok()
            .and_then(|value| value.get("domains").and_then(Value::as_array).cloned());
        if let Some(domains) = domains {
            let supports = |name: &str| {
                domains
                    .iter()
                    .any(|domain| domain.get("name").and_then(Value::as_str) == Some(name))
            };
            if supports("Page") {
                report.capabilities.launch = robot_core::Capability::available(
                    "Page domain is reachable for browser navigation; later navigation can fail",
                );
                let layout = self
                    .cdp(&target, "Page.getLayoutMetrics", json!({}))
                    .await
                    .is_ok();
                if layout {
                    report.capabilities.screenshot = robot_core::Capability::available(
                        "Page domain and layout query are responsive; capture can still fail",
                    );
                }
            }
            if supports("Runtime") && self.query_dom(&target).await.is_ok() {
                report.capabilities.elements = robot_core::Capability::available(
                    "Read-only page element query succeeded; later queries can fail",
                );
                if supports("Input") {
                    report.capabilities.input = robot_core::Capability::available(
                        "Input domain and page query are responsive; later input can fail",
                    );
                }
            }
            if supports("Runtime") && supports("Network") {
                let browser_ready = match self.running_browser_endpoint().await {
                    Ok(endpoint) => match connection::BrowserConnection::connect(&endpoint).await {
                        Ok(mut browser) => browser
                            .request("Browser.getVersion", json!({}), None)
                            .await
                            .is_ok(),
                        Err(_) => false,
                    },
                    Err(_) => false,
                };
                report.capabilities.observation = if browser_ready {
                    robot_core::Capability::available(
                        "Browser connection and Runtime/Network domains are reachable; observer attachment and event delivery can still fail",
                    )
                } else {
                    robot_core::Capability::blocked(
                        "Check browser-level DevTools connectivity for observation",
                    )
                };
            }
        } else {
            let blocked =
                robot_core::Capability::blocked("Check the selected page's DevTools connection");
            report.capabilities.input = blocked.clone();
            report.capabilities.screenshot = blocked.clone();
            report.capabilities.elements = blocked.clone();
            report.capabilities.screen_recording = blocked.clone();
            report.capabilities.observation = blocked;
        }
        Ok(report)
    }

    async fn start_observation(
        &self,
        id: &str,
        options: robot_core::ObservationOptions,
    ) -> Result<String, RobotError> {
        self.begin_observer(id, options).await
    }
    async fn poll_observation(
        &self,
        id: &str,
        observation: &str,
        after: u64,
        limit: usize,
    ) -> Result<robot_core::ObservationBatch, RobotError> {
        self.read_observer(id, observation, after, limit).await
    }
    async fn stop_observation(&self, id: &str, observation: &str) -> Result<(), RobotError> {
        self.end_observer(id, observation).await
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
        let reference = context.element_ref.as_ref().ok_or_else(|| {
            RobotError::InvalidInput("Inspect the DOM element again before scrolling it".into())
        })?;
        if !dx.is_finite() || !dy.is_finite() {
            return Err(RobotError::InvalidInput("Invalid scroll delta".into()));
        }
        let target = self.page(id).await?;
        let script = format!(
            "({})(document, {}, {}, {}, {})",
            include_str!("../../robot-ios-real/webview-helper/dom-scroll.js"),
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            serde_json::to_string(reference).unwrap(),
            dx,
            dy
        );
        self.eval_in_context(&target, context, &script).await
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
        let reference = context.element_ref.as_ref().ok_or_else(|| {
            RobotError::InvalidInput("Inspect the DOM element again before activating it".into())
        })?;
        let target = self.page(id).await?;
        let script = format!(
            "({})(document, {}, {})",
            include_str!("../../robot-ios-real/webview-helper/dom-tap.js"),
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            serde_json::to_string(reference).unwrap()
        );
        self.eval_in_context(&target, context, &script).await
    }
    async fn negotiate_native_video(
        &self,
        id: &str,
        offer: &str,
    ) -> Result<Option<robot_core::NativeVideoStream>, RobotError> {
        let target = self.page(id).await?;
        let capabilities = self.cdp(&target, "Runtime.evaluate", json!({"expression":"Boolean(isSecureContext && navigator.mediaDevices?.getDisplayMedia && window.CropTarget)","returnByValue":true})).await?;
        if capabilities["result"]["value"] != true {
            return Ok(None);
        }
        // A user denial is returned; it must never silently enable fallback capture.
        self.start_live(id, offer).await
    }
    async fn start_video_stream(&self, id: &str) -> Result<robot_core::VideoStream, RobotError> {
        self.start_screencast(id).await
    }
    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        Ok(json!({"url":self.page(id).await?.url}))
    }
    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        let target = self.page(id).await?;
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
        self.cdp(
            &target,
            "Input.dispatchMouseEvent",
            json!({"type":"mouseWheel","x":at.x,"y":at.y,"deltaX":dx,"deltaY":dy}),
        )
        .await
        .map(|_| ())
    }
    async fn restart_device(&self, id: &str) -> Result<(), RobotError> {
        if self.test_proxy.lock().await.is_some() {
            return Err(RobotError::InvalidInput(
                "start a new MCP session to restart Chrome with a test proxy".into(),
            ));
        }
        if id != "chrome:browser" {
            self.page(id).await?;
        }
        let _ = self.close_session().await;
        self.start_browser("chrome:browser").await
    }
    async fn clear_text(&self, id: &str) -> Result<(), RobotError> {
        let target = self.page(id).await?;
        let response = self.cdp(&target, "Runtime.evaluate", json!({"expression": "(() => { const e=document.activeElement; if (!e || e.disabled || e.readOnly || !(e.isContentEditable || /^(INPUT|TEXTAREA)$/.test(e.tagName))) throw new Error('Focus an editable field first'); e.focus(); document.execCommand('selectAll',false,null); document.execCommand('delete',false,null); return true; })()", "returnByValue":true})).await?;
        if response.get("exceptionDetails").is_some() {
            return Err(RobotError::InvalidInput(
                "Focus an editable field before clearing text".into(),
            ));
        }
        Ok(())
    }

    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        let mut robot = Self::new(self.debug_address.clone());
        robot.discovery_file = self.discovery_file.clone();
        robot.browser_connection = Arc::clone(&self.browser_connection);
        robot.managed_browser = self.managed_browser.clone();
        robot.artifact_dir = self.artifact_dir.clone();
        robot.ffmpeg_path = self.ffmpeg_path.clone();
        Some(Arc::new(robot))
    }

    async fn configure_web_proxy(
        &self,
        id: &str,
        config: robot_core::WebProxyConfig,
    ) -> Result<(), RobotError> {
        let _boot = self.boot_lock.lock().await;
        if id != "chrome:browser" || self.browser_session.lock().await.is_some() {
            return Err(RobotError::InvalidInput(
                "configure the test proxy on chrome:browser before starting the owned context"
                    .into(),
            ));
        }
        let mut proxy = self.test_proxy.lock().await;
        if proxy.is_some() {
            return Err(RobotError::InvalidInput(
                "test proxy already configured for this session".into(),
            ));
        }
        *proxy = Some(test_proxy::TestProxy::start(config).await?);
        Ok(())
    }

    async fn set_web_fault(
        &self,
        id: &str,
        fault: Option<robot_core::WebFault>,
    ) -> Result<robot_core::WebFaultTransition, RobotError> {
        if id != "chrome:browser" {
            self.page(id).await?;
        }
        self.test_proxy
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| {
                RobotError::InvalidInput("no test proxy configured on this Chrome session".into())
            })?
            .set(fault)
    }

    async fn web_fault_state(&self, id: &str) -> Result<robot_core::WebFaultState, RobotError> {
        if id != "chrome:browser" {
            self.page(id).await?;
        }
        self.test_proxy
            .lock()
            .await
            .as_ref()
            .map(test_proxy::TestProxy::state)
            .ok_or_else(|| {
                RobotError::InvalidInput("no test proxy configured on this Chrome session".into())
            })
    }

    async fn close_session(&self) -> Result<(), RobotError> {
        self.end_all_observers().await;
        let recordings: Vec<_> = self
            .recordings
            .lock()
            .await
            .drain()
            .map(|(_, r)| r)
            .collect();
        let mut first = None;
        for recording in recordings {
            let _ = recording.stop.send(());
            match recording.task.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first.is_none() {
                        first = Some(e);
                    }
                }
                Err(e) => {
                    if first.is_none() {
                        first = Some(RobotError::CommandFailed(e.to_string()));
                    }
                }
            }
        }
        let _boot = self.boot_lock.lock().await;
        if let Some(mut session) = self.browser_session.lock().await.take() {
            if let Err(e) = session.close().await {
                if first.is_none() {
                    first = Some(e);
                }
            }
        }
        self.test_proxy.lock().await.take();
        first.map_or(Ok(()), Err)
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        // Chrome is optional. An inactive DevTools endpoint must not stop the
        // mobile and desktop drivers from listing their own devices.
        let targets = match self.targets().await {
            Ok(targets) => targets,
            Err(_) => return Ok(vec![self.offline_browser()]),
        };
        let devices: Vec<_> = targets
            .into_iter()
            .filter(|target| target.kind == "page" && target.web_socket_debugger_url.is_some())
            .map(|target| RobotDevice {
                id: device_id(&target),
                name: if target.title.is_empty() {
                    target.url
                } else {
                    target.title
                },
                platform: Platform::Chrome,
                state: DeviceState::Available,
            })
            .collect();
        Ok(if devices.is_empty() {
            vec![self.offline_browser()]
        } else {
            devices
        })
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        self.start_browser(device_id).await
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        let target = self.page(device_id).await?;
        self.capture_screenshot(&target).await
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        let target = self.page(device_id).await?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create Chrome recording directory: {error}"
                ))
            })?;
        let path = self.artifact_dir.join(format!(
            "chrome-recording-{}.mp4",
            RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut recordings = self.recordings.lock().await;
        if recordings
            .get(device_id)
            .is_some_and(|recording| !recording.task.is_finished())
        {
            return Err(RobotError::InvalidInput(format!(
                "a Chrome recording is already in progress for {device_id}"
            )));
        }
        recordings.remove(device_id);
        let mut encoder = ffmpeg::command(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "image2pipe",
                "-framerate",
                "10",
                "-vcodec",
                "png",
                "-i",
                "-",
                "-an",
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start ffmpeg for Chrome recording (set DEVICE_BOT_FFMPEG_PATH if needed): {error}"
                ))
            })?;
        let mut stdin = encoder.stdin.take().ok_or_else(|| {
            RobotError::CommandFailed("ffmpeg did not provide a writable video input".into())
        })?;
        let (stop, mut stopped) = oneshot::channel();
        let robot = self.clone();
        let task = tokio::spawn(async move {
            let mut frames = interval(Duration::from_millis(100));
            frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut capture_error = None;
            loop {
                tokio::select! { _ = &mut stopped => break, _ = frames.tick() => {} }
                let frame = tokio::select! {
                    _ = &mut stopped => break,
                    result = robot.capture_screenshot(&target) => result,
                };
                match frame {
                    Ok(frame) => {
                        let result = tokio::select! {
                            _ = &mut stopped => break,
                            result = stdin.write_all(&frame) => result,
                        };
                        if let Err(error) = result {
                            capture_error = Some(RobotError::CommandFailed(format!(
                                "could not send a Chrome frame to ffmpeg: {error}"
                            )));
                            break;
                        }
                    }
                    Err(error) => {
                        capture_error = Some(error);
                        break;
                    }
                }
            }
            drop(stdin);
            let output = encoder.wait_with_output().await.map_err(|error| {
                RobotError::CommandFailed(format!("could not finalize Chrome recording: {error}"))
            })?;
            if let Some(error) = capture_error {
                return Err(error);
            }
            if !output.status.success() {
                return Err(RobotError::CommandFailed(format!(
                    "ffmpeg could not encode Chrome recording: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            Ok(())
        });
        recordings.insert(
            device_id.into(),
            ActiveRecording {
                path: path.clone(),
                stop,
                task,
            },
        );
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.page(device_id).await?;
        let recording = self
            .recordings
            .lock()
            .await
            .remove(device_id)
            .ok_or_else(|| {
                RobotError::InvalidInput(format!(
                    "no Chrome recording is in progress for {device_id}"
                ))
            })?;
        let _ = recording.stop.send(());
        recording.task.await.map_err(|error| {
            RobotError::CommandFailed(format!("Chrome recording task failed: {error}"))
        })??;
        if !recording.path.is_file() {
            return Err(RobotError::CommandFailed(
                "ffmpeg did not create a Chrome recording artifact".into(),
            ));
        }
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.dispatch_tap(&target, point).await
    }

    async fn pointer_gesture(
        &self,
        id: &str,
        point: Point,
        gesture: robot_core::PointerGesture,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if gesture == robot_core::PointerGesture::Tap {
            return self.tap_in_context(id, point, context).await;
        }
        let target = self.page(id).await?;
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
        let point = if let Some(context) = context.filter(|value| !value.is_empty()) {
            self.checked_pointer_points(&target, context, &[[0.5, 0.5]])
                .await?[0]
        } else {
            point
        };
        if ![point.x, point.y]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.)
        {
            return Err(RobotError::InvalidInput(
                "Invalid Chrome pointer coordinates".into(),
            ));
        }
        self.dispatch_pointer_gesture(&target, point, gesture).await
    }

    async fn launch_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "Chrome pages cannot launch native applications".into(),
        ))
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        _app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.cdp(&target, "Page.navigate", json!({ "url": url }))
            .await
            .map(|_| ())
    }

    async fn reset_web_document(&self, device_id: &str, url: &str) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        let navigation = self
            .cdp(&target, "Page.navigate", json!({"url":url}))
            .await?;
        if let Some(error) = navigation["errorText"].as_str() {
            return Err(RobotError::CommandFailed(format!(
                "Could not navigate to entry: {error}"
            )));
        }
        if !navigation["loaderId"].is_string() {
            // Chromium keeps the current JS realm for same-document hash changes.
            self.cdp(&target, "Page.reload", json!({})).await?;
        }
        Ok(())
    }

    async fn web_viewport(&self, device_id: &str) -> Result<robot_core::WebViewport, RobotError> {
        let target = self.page(device_id).await?;
        let metrics = self
            .cdp(&target, "Page.getLayoutMetrics", json!({}))
            .await?;
        let dimension = |field: &str| {
            metrics
                .pointer(field)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| {
                    RobotError::CommandFailed(format!(
                        "Chrome did not expose viewport metric {field}"
                    ))
                })
        };
        let width = dimension("/cssVisualViewport/clientWidth")?;
        let height = dimension("/cssVisualViewport/clientHeight")?;
        Ok(robot_core::WebViewport {
            width,
            height,
            content_width: dimension("/cssContentSize/width")?.max(width),
            content_height: dimension("/cssContentSize/height")?.max(height),
        })
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.cdp(&target, "Input.insertText", json!({ "text": text }))
            .await
            .map(|_| ())
    }

    async fn drag(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration: u64,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        if !(1..=robot_core::MAX_DRAG_DURATION_MS).contains(&duration) {
            return Err(RobotError::InvalidInput(
                "Drag duration must be 1..3000 ms".into(),
            ));
        }
        let guard = self.drag_lock.clone().lock_owned().await;
        let target = self.page(device_id).await?;
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
        let (start, end) = if let Some(context) = context.filter(|value| !value.is_empty()) {
            let fractions = [[start.x, start.y], [end.x, end.y]];
            if fractions.iter().flatten().any(|v| !(0. ..=1.).contains(v)) {
                return Err(RobotError::InvalidInput(
                    "DOM drag coordinates must be fractions in 0..1".into(),
                ));
            }
            let points = self
                .checked_pointer_points(&target, context, &fractions)
                .await?;
            (points[0], points[1])
        } else {
            (start, end)
        };
        let metrics = self
            .cdp(&target, "Page.getLayoutMetrics", json!({}))
            .await?;
        let viewport = &metrics["cssVisualViewport"];
        let width = viewport["clientWidth"].as_f64().unwrap_or_default();
        let height = viewport["clientHeight"].as_f64().unwrap_or_default();
        if (start.x == end.x && start.y == end.y)
            || [start, end]
                .iter()
                .any(|p| !(0. ..width).contains(&p.x) || !(0. ..height).contains(&p.y))
        {
            return Err(RobotError::InvalidInput(
                "Drag endpoints must differ and be inside the Chrome viewport".into(),
            ));
        }
        // Once contact begins, finish or cancel it even if the caller disconnects.
        let this = self.clone();
        tokio::spawn(async move {
            let _guard = guard;
            this.dispatch_touch_drag(&target, start, end, duration)
                .await
        })
        .await
        .map_err(|error| RobotError::CommandFailed(format!("Chrome drag task failed: {error}")))?
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        _duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.cdp(
            &target,
            "Input.dispatchMouseEvent",
            json!({"type":"mouseWheel", "x":start.x,"y":start.y,
            "deltaX":start.x-end.x,"deltaY":start.y-end.y}),
        )
        .await
        .map(|_| ())
    }

    async fn install_app(&self, device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "app installation is not supported by the Chrome robot".into(),
        ))
    }

    async fn uninstall_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "app removal is not supported by the Chrome robot".into(),
        ))
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        let target = self.page(device_id).await?;
        let result = self.cdp(&target, "Runtime.evaluate", json!({
            "expression": "typeof globalThis.__deviceBotFlutterSnapshot === 'function' ? globalThis.__deviceBotFlutterSnapshot() : null",
            "awaitPromise": true, "returnByValue": true,
        })).await?;
        let Some(snapshot) = result.pointer("/result/value").and_then(Value::as_str) else {
            return Ok(vec![]);
        };
        let snapshot: Value = serde_json::from_str(snapshot)
            .map_err(|_| RobotError::CommandFailed("Invalid Flutter web snapshot".into()))?;
        if snapshot["version"] != 1 {
            return Err(RobotError::CommandFailed(
                "Unsupported Flutter web bridge version".into(),
            ));
        }
        robot_flutter_bridge::parse_elements(&snapshot["elements"])
            .ok_or_else(|| RobotError::CommandFailed("Invalid Flutter web elements".into()))
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        let target = self.page(device_id).await?;
        let mut elements = self.query_dom(&target).await?;
        // A child-frame failure keeps the root document usable.
        if let Ok(children) = self.query_child_frames(&target, false).await {
            elements.extend(children);
        }
        Ok(elements)
    }

    async fn list_elements_query(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        query.reject_keyboard_exclusion()?;
        if let Some(context) = query.web_context.clone() {
            context.validate()?;
            if !context.is_empty() {
                let target = self.page(device_id).await?;
                self.eval_in_context(&target, &context, "true").await?;
                let mut elements = self
                    .query_child_frames(&target, context.frame_path.is_empty())
                    .await?;
                for element in &mut elements {
                    if let Some(actual) = element.web_context.as_mut() {
                        actual.page = context.page.clone();
                    }
                }
                let scoped = ElementQuery {
                    include_frames: true,
                    ..query
                };
                return Ok(apply_element_query(elements, &scoped));
            }
        }
        if query.include_frames {
            let target = self.page(device_id).await?;
            let mut elements = self.query_dom(&target).await?;
            elements.extend(self.query_child_frames(&target, false).await?);
            return Ok(apply_element_query(elements, &query));
        }
        let target = self.page(device_id).await?;
        let elements = self.query_dom(&target).await?;
        Ok(apply_element_query(elements, &query))
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
        context.validate()?;
        let target = self.page(device_id).await?;
        let script = format!(
            "({})(document, {}, {}, \"type\", {})",
            include_str!("../../robot-ios-real/webview-helper/dom-edit.js"),
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            serde_json::to_string(&context.element_ref).unwrap(),
            serde_json::to_string(text).unwrap()
        );
        self.eval_in_context(&target, context, &script).await
    }

    async fn clear_text_in_context(
        &self,
        device_id: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        let Some(context) = context.filter(|context| !context.is_empty()) else {
            return self.clear_text(device_id).await;
        };
        context.validate()?;
        let target = self.page(device_id).await?;
        let script = format!(
            "({})(document, {}, {}, \"clear\", \"\")",
            include_str!("../../robot-ios-real/webview-helper/dom-edit.js"),
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            serde_json::to_string(&context.element_ref).unwrap()
        );
        self.eval_in_context(&target, context, &script).await
    }
}

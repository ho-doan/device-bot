//! Portable device operations backed by native platform modules.

#[cfg(target_os = "macos")]
use std::time::Duration;
use std::{
    process::Stdio,
    sync::{Arc, atomic::Ordering},
};

use async_trait::async_trait;
use robot_core::{
    AppWindow, DeviceState, ElementQuery, Platform, Point, Robot, RobotDevice, RobotError,
    RunningApp, UiElement, UiTarget,
};
use serde_json::{Value, json};
use tokio::process::Command;

#[cfg(target_os = "macos")]
use super::{
    AXIsProcessTrusted, AXUIElementGetPid, CGPreflightScreenCaptureAccess,
    capability_ffmpeg_listing, mac_ax_readiness, mac_display_readiness, native_accessibility_tap,
    native_frontmost_application, native_listed_windows, running_apps_from_windows,
    wait_for_activation, windows_for_process,
};
use super::{
    ActiveRecording, DEVICE_ID, MacosRobot, RECORDING_SEQUENCE, live, pointer, recording,
    request_screen_recording_permission,
};

#[async_trait]
impl Robot for MacosRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        self.require_device(id)?;
        let mut report = robot_core::CapabilityReport::new(id);
        report.capabilities.discovery =
            robot_core::Capability::available("macOS host driver is present");
        report.capabilities.observation = robot_core::Capability::unsupported(
            "Native application log and network observation is not implemented",
        );
        #[cfg(target_os = "macos")]
        {
            // These APIs never request permission or display a system prompt.
            let ax = unsafe { AXIsProcessTrusted() };
            let capture = unsafe { CGPreflightScreenCaptureAccess() };
            let ax_ready = if ax {
                self.accessibility_elements(ElementQuery {
                    max_results: Some(1),
                    ..Default::default()
                })
                .await
                .is_ok()
            } else {
                false
            };
            let ax_status = mac_ax_readiness(ax, ax_ready);
            report.capabilities.input = ax_status.clone();
            report.capabilities.elements = ax_status;
            if id != DEVICE_ID {
                let unknown = robot_core::Capability::unknown(
                    "Window-scoped target readiness is not verified by the host accessibility probe",
                );
                report.capabilities.input = unknown.clone();
                report.capabilities.elements = unknown;
                report.capabilities.launch =
                    robot_core::Capability::unsupported("Window-scoped targets cannot launch apps");
            }
            let display_ready = if capture {
                self.desktop_displays().await.is_ok()
            } else {
                false
            };
            let display_status = mac_display_readiness(capture, display_ready);
            report.capabilities.screenshot = display_status.clone();
            report.capabilities.screen_recording = display_status;
            if capture && display_ready {
                let (devices, encoders) = tokio::join!(
                    capability_ffmpeg_listing(&self.ffmpeg_path, "-devices"),
                    capability_ffmpeg_listing(&self.ffmpeg_path, "-encoders")
                );
                report.capabilities.screen_recording = if devices
                    .as_deref()
                    .is_some_and(|s| s.contains("avfoundation"))
                    && encoders.as_deref().is_some_and(|s| s.contains("libx264"))
                {
                    robot_core::Capability::available(
                        "Screen permission, active display and ffmpeg AVFoundation/libx264 support are present; recording can still fail",
                    )
                } else {
                    robot_core::Capability::blocked(
                        "Install ffmpeg with AVFoundation and libx264 support or configure DEVICE_BOT_FFMPEG_PATH",
                    )
                };
            }
            if self.recording.lock().await.is_some() {
                report.capabilities.screen_recording = robot_core::Capability::blocked(
                    "Stop the current macOS recording before starting another",
                );
            }
        }
        Ok(report)
    }

    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        self.live_context(id).await
    }

    async fn start_video_stream(&self, id: &str) -> Result<robot_core::VideoStream, RobotError> {
        self.start_live(id).await
    }

    async fn clear_text(&self, id: &str) -> Result<(), RobotError> {
        self.require_device(id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(id)?.is_some() {
            let target = self.live_guard(id, &[]).await?;
            if self.live_edit_text(target, "", true) {
                return Ok(());
            }
        } else {
            self.require_input_target().await?;
        }
        self.apple_script("tell application \"System Events\"\n keystroke \"a\" using command down\n key code 51\nend tell", &[]).await
    }

    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        self.require_device(id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(id)?.is_some() {
            self.live_guard(id, &[at]).await?;
        } else {
            self.require_input_target().await?;
        }
        live::scroll(at, dx, dy)
    }
    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        let mut session = self.clone();
        session.active_application = Arc::default();
        session.active_process_id = Arc::default();
        session.active_window_id = Arc::default();
        session.live_geometry = Arc::default();
        #[cfg(target_os = "macos")]
        {
            session.flutter_windows = Arc::default();
            session.fixture = Arc::default();
        }
        session.recording = Arc::default();
        Some(Arc::new(session))
    }

    async fn close_session(&self) -> Result<(), RobotError> {
        let mut slot = self.recording.lock().await;
        if let Some(recording) = slot.take() {
            recording::finish(recording, std::time::Duration::from_secs(15)).await?;
        }
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        if !cfg!(target_os = "macos") {
            return Ok(vec![]);
        }
        Ok(vec![RobotDevice {
            id: DEVICE_ID.into(),
            name: "This Mac".into(),
            platform: Platform::Macos,
            state: DeviceState::Available,
        }])
    }

    async fn list_running_apps(&self, device_id: &str) -> Result<Vec<RunningApp>, RobotError> {
        self.require_device(device_id)?;
        #[cfg(target_os = "macos")]
        {
            return Ok(running_apps_from_windows(&native_listed_windows()?));
        }
        #[cfg(not(target_os = "macos"))]
        Ok(vec![])
    }

    async fn list_app_windows(
        &self,
        device_id: &str,
        process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        self.require_device(device_id)?;
        #[cfg(target_os = "macos")]
        {
            return Ok(windows_for_process(&native_listed_windows()?, process_id));
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = process_id;
            Ok(vec![])
        }
    }

    async fn select_ui_target(&self, device_id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return Err(RobotError::InvalidInput(
                "A macOS Live target is immutable; choose another app/window on macos-host".into(),
            ));
        }
        if let Some(process_id) = target.process_id {
            if process_id == 0 || process_id > i32::MAX as u32 {
                return Err(RobotError::InvalidInput(
                    "processId must be a positive 32-bit integer".into(),
                ));
            }
            if *self.active_process_id.lock().await != Some(process_id as i32) {
                *self.active_window_id.lock().await = None;
                *self.active_application.lock().await = None;
            }
            *self.active_process_id.lock().await = Some(process_id as i32);
        }
        if let Some(window_id) = target.window_id {
            *self.active_window_id.lock().await = Some(window_id);
        }
        if self.active_process_id.lock().await.is_none() {
            return Err(RobotError::InvalidInput(
                "select a running app before querying elements".into(),
            ));
        }
        Ok(())
    }

    async fn activate_ui_target(
        &self,
        device_id: &str,
        target: UiTarget,
    ) -> Result<(), RobotError> {
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            self.require_device(device_id)?;
            self.live_guard(device_id, &[]).await?;
            return Ok(());
        }
        self.select_ui_target(device_id, target.clone()).await?;
        let Some(process_id) = *self.active_process_id.lock().await else {
            return Err(RobotError::InvalidInput(
                "select a running app before activating it".into(),
            ));
        };
        self.apple_script(
            "on run argv\n tell application \"System Events\" to set frontmost of (first process whose unix id is (item 1 of argv as integer)) to true\nend run",
            &[process_id.to_string()],
        )
        .await?;
        #[cfg(target_os = "macos")]
        {
            return wait_for_activation(|| self.require_input_target(), Duration::from_secs(2))
                .await;
        }
        #[cfg(not(target_os = "macos"))]
        Ok(())
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        self.require_device(device_id)?;
        let output = self
            .run(
                "system_profiler",
                &["SPApplicationsDataType".into(), "-json".into()],
            )
            .await?;
        let report = serde_json::from_slice::<Value>(&output).map_err(|error| {
            RobotError::CommandFailed(format!("system_profiler returned invalid JSON: {error}"))
        })?;
        let apps = report
            .get("SPApplicationsDataType")
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(json!({
            "platform": "macos",
            "deviceId": device_id,
            "apps": apps,
        }))
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.take_screenshot_with_options(device_id, &robot_core::ScreenshotOptions::default())
            .await
    }

    async fn take_screenshot_with_options(
        &self,
        device_id: &str,
        options: &robot_core::ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        if let Some(target) = live::Target::parse(device_id)? {
            if options.app_id.is_some() || options.display_id.is_some() {
                return Err(RobotError::InvalidInput(
                    "A window-scoped screenshot cannot select a different app or display".into(),
                ));
            }
            return self.capture_window(target).await;
        }
        self.capture_desktop(options).await
    }

    async fn list_displays(
        &self,
        device_id: &str,
    ) -> Result<Vec<robot_core::DesktopDisplay>, RobotError> {
        self.require_device(device_id)?;
        self.desktop_displays().await
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        if !request_screen_recording_permission() {
            return Err(RobotError::CommandFailed(
                "Screen Recording permission is required. macOS has been asked to grant it; approve the prompt, then retry the recording.".into(),
            ));
        }
        let mut recording = self.recording.lock().await;
        if recording.is_some() {
            return Err(RobotError::InvalidInput(
                "a macOS screen recording is already in progress".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create recording directory: {error}"))
            })?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = self.artifact_dir.join(format!(
            "macos-recording-{stamp}-{}-{}.mp4",
            std::process::id(),
            RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // Select AVFoundation's named desktop source, never its numeric device
        // index: indexes can point at a connected Continuity camera. Unlike
        // `screencapture -v`, ffmpeg finalizes the MP4 on graceful quit.
        let input = format!("{}:none", Self::screen_capture_device());
        let mut child = Command::new(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "avfoundation",
                "-capture_cursor",
                "1",
                "-framerate",
                "30",
                "-i",
            ])
            .arg(input)
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-progress",
                "pipe:1",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start ffmpeg screen recording (run --install-chrome-recording-tools or set DEVICE_BOT_FFMPEG_PATH): {error}"
                ))
            })?;
        recording::ready(&mut child, std::time::Duration::from_secs(10)).await?;
        *recording = Some(ActiveRecording {
            path: path.clone(),
            child,
        });
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        let mut slot = self.recording.lock().await;
        let Some(recording) = slot.take() else {
            return Err(RobotError::InvalidInput(
                "no macOS screen recording is in progress".into(),
            ));
        };
        recording::finish(recording, std::time::Duration::from_secs(15)).await
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            let target = self.live_guard(device_id, &[point]).await?;
            #[cfg(target_os = "macos")]
            {
                if self.flutter_windows.lock().await.contains(device_id) {
                    return pointer::gesture(point, robot_core::PointerGesture::Tap).await;
                }
                return native_accessibility_tap(point, Some(target.pid));
            }
            #[cfg(not(target_os = "macos"))]
            let _ = target;
        }
        self.accessibility_tap(point).await
    }

    async fn pointer_gesture(
        &self,
        device_id: &str,
        point: Point,
        gesture: robot_core::PointerGesture,
        context: Option<&robot_core::WebContext>,
    ) -> Result<(), RobotError> {
        if gesture == robot_core::PointerGesture::Tap {
            return self.tap_in_context(device_id, point, context).await;
        }
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported("macOS DOM pointer gestures".into()));
        }
        self.require_device(device_id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            self.live_guard(device_id, &[point]).await?;
        } else {
            self.require_input_target().await?;
        }
        pointer::gesture(point, gesture).await
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return Err(RobotError::Unsupported(
                "A Live window target is fixed; choose another window to change apps".into(),
            ));
        }
        let _guard = self.capture_lock.lock().await;
        if let Ok(arguments) = std::env::var("DEVICE_BOT_MACOS_APP_ARGS") {
            let arguments = arguments.split_whitespace().collect::<Vec<_>>();
            if !arguments.is_empty() {
                // `open -a` ignores `--args` when the application is already
                // running. Start a fresh instance so flags such as Chrome's
                // `--force-renderer-accessibility` take effect.
                let mut launch_args =
                    vec!["-n".into(), "-a".into(), app_id.into(), "--args".into()];
                launch_args.extend(arguments.into_iter().map(str::to_owned));
                self.run(&self.open_path, &launch_args).await?;
            }
        }
        // AppKit resolves bundle IDs, app names and paths without Apple Events.
        let mut last_error = None;
        for _ in 0..20 {
            match self.focus_command("activate", app_id).await {
                Ok(pid) => {
                    *self.active_application.lock().await = Some(app_id.into());
                    *self.active_process_id.lock().await = Some(pid);
                    return Ok(());
                }
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Err(last_error
            .unwrap_or_else(|| RobotError::CommandFailed(format!("could not activate {app_id}"))))
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _ = (url, app_id);
        Err(RobotError::Unsupported(
            "URL navigation belongs to the Chrome DevTools robot; use a chrome:<target-id> device"
                .into(),
        ))
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            let target = self.live_guard(device_id, &[]).await?;
            // AXSelectedText inserts a newline into the value; it does not send
            // the Return event that submits an editor (for example Chrome's URL bar).
            if text == "\n" {
                return self
                    .apple_script("tell application \"System Events\" to key code 36", &[])
                    .await;
            }
            if text != "\u{1b}" && self.live_edit_text(target, text, false) {
                return Ok(());
            }
            return self.apple_script("on run argv\n tell application \"System Events\" to keystroke (item 1 of argv)\nend run", &[text.into()]).await;
        }
        self.require_input_target().await?;
        self.apple_script(
            "on run argv\n set targetApp to item 1 of argv\n set textToType to item 2 of argv\n if targetApp is not \"\" then tell application targetApp to activate\n tell application \"System Events\"\n  if textToType is character id 27 then\n   key code 53\n  else\n   set AppleScript's text item delimiters to linefeed\n   set parts to text items of textToType\n   repeat with index from 1 to count of parts\n    keystroke (item index of parts)\n    if index is less than count of parts then key code 36\n   end repeat\n  end if\n end tell\nend run",
            &[
                self.active_application.lock().await.clone().unwrap_or_default(),
                text.into(),
            ],
        ).await
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            self.live_guard(device_id, &[start, end]).await?;
            return pointer::drag(start, end, duration_ms.unwrap_or(300)).await;
        }
        self.require_input_target().await?;
        pointer::drag(start, end, duration_ms.unwrap_or(300)).await
    }

    async fn install_app(&self, device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app installation is not supported by the macOS robot".into(),
        ))
    }

    async fn uninstall_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app removal is not supported by the macOS robot".into(),
        ))
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        if let Some(target) = live::Target::parse(device_id)? {
            return self.live_flutter_elements(device_id, target).await;
        }
        if let Some(elements) = robot_flutter_bridge::configured(device_id).await {
            return Ok(elements);
        }
        #[cfg(target_os = "macos")]
        {
            let pid = {
                let application =
                    native_frontmost_application(*self.active_process_id.lock().await)?;
                let mut pid: i32 = 0;
                let status = unsafe { AXUIElementGetPid(application.0, &mut pid) };
                if status != 0 || pid <= 0 {
                    return Ok(vec![]);
                }
                pid as u32
            };
            return Ok(robot_flutter_bridge::for_process(pid)
                .await
                .unwrap_or_default());
        }
        #[cfg(not(target_os = "macos"))]
        Ok(vec![])
    }

    #[cfg(target_os = "macos")]
    async fn bind_window_fixture(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.bind_fixture(device_id, app_id).await
    }

    #[cfg(target_os = "macos")]
    async fn restart_window_fixture(&self, device_id: &str) -> Result<String, RobotError> {
        self.restart_fixture(device_id).await
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_query(device_id, ElementQuery::default())
            .await
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
        query.reject_web_context(device_id)?;
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return self.live_elements(device_id, query).await;
        }
        self.accessibility_elements(query).await
    }
}

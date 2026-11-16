//! Native macOS desktop driver.
//!
//! Input is performed through `System Events`, so macOS will require the MCP
//! server process to have Accessibility permission before pointer or keyboard
//! commands can succeed.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(target_os = "macos")]
use std::{ptr, time::Duration};

use async_trait::async_trait;
use robot_core::{
    AppWindow, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, RunningApp, UiElement,
    UiTarget,
};
use serde_json::{Value, json};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

#[cfg(target_os = "macos")]
mod accessibility;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(any(target_os = "macos", test))]
mod windows;

#[cfg(target_os = "macos")]
use accessibility::{
    NativeAxAttributes, native_accessibility_elements, native_accessibility_tap,
    native_bounds_contains, native_frontmost_application, native_required_attribute,
};
#[cfg(target_os = "macos")]
use ffi::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElementGetPid, CFDictionaryCreate,
    CFRelease, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    kAXTrustedCheckOptionPrompt, kCFBooleanTrue,
};
#[cfg(target_os = "macos")]
use windows::{native_listed_windows, running_apps_from_windows, windows_for_process};

mod live;
pub use live::{prepare_window_capture, start_window_capture};

const DEVICE_ID: &str = "macos-host";
#[cfg(target_os = "macos")]
const NATIVE_FILTER_BUDGET: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
async fn wait_for_activation<F, Fut>(mut check: F, timeout: Duration) -> Result<(), RobotError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), RobotError>>,
{
    tokio::time::timeout(timeout, async {
        loop {
            if check().await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| {
        RobotError::CommandFailed(
            "macOS did not activate the selected process; retry Activate".into(),
        )
    })
}

static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Requests the macOS Screen Recording permission when it has not yet been
/// granted. macOS owns the prompt and can require a retry after approval.
pub fn request_screen_recording_permission() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Prompts for Accessibility permission when the MCP process is not trusted.
/// macOS owns the consent flow, so callers must retry after the user approves.
pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            ptr::null(),
            ptr::null(),
        );
        if options.is_null() {
            return false;
        }
        let trusted = AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
        trusted
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[derive(Debug)]
struct ActiveRecording {
    path: PathBuf,
    child: Child,
}

#[derive(Debug, Clone)]
pub struct MacosRobot {
    artifact_dir: PathBuf,
    ffmpeg_path: String,
    osascript_path: String,
    open_path: String,
    screencapture_path: String,
    recording: Arc<Mutex<Option<ActiveRecording>>>,
    active_application: Arc<Mutex<Option<String>>>,
    active_process_id: Arc<Mutex<Option<i32>>>,
    active_window_id: Arc<Mutex<Option<String>>>,
    capture_lock: Arc<Mutex<()>>,
    live_geometry: Arc<Mutex<std::collections::HashMap<String, live::Geometry>>>,
}

impl MacosRobot {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifact_dir: artifact_dir.into(),
            ffmpeg_path: Self::ffmpeg_path(),
            osascript_path: "osascript".into(),
            open_path: "open".into(),
            screencapture_path: "screencapture".into(),
            recording: Arc::new(Mutex::new(None)),
            active_application: Arc::new(Mutex::new(None)),
            active_process_id: Arc::default(),
            active_window_id: Arc::default(),
            capture_lock: Arc::default(),
            live_geometry: Arc::default(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()))
    }

    fn ffmpeg_path() -> String {
        std::env::var("DEVICE_BOT_FFMPEG_PATH").unwrap_or_else(|_| {
            [
                "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
                "/usr/local/opt/ffmpeg/bin/ffmpeg",
            ]
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
            .map_or_else(|| "ffmpeg".into(), |path| (*path).into())
        })
    }

    fn screen_capture_device() -> String {
        std::env::var("DEVICE_BOT_MACOS_SCREEN_DEVICE")
            .unwrap_or_else(|_| "Capture screen 0".into())
    }

    fn require_device(&self, device_id: &str) -> Result<(), RobotError> {
        if device_id != DEVICE_ID && live::Target::parse(device_id)?.is_none() {
            return Err(RobotError::DeviceNotFound(device_id.into()));
        }
        if !cfg!(target_os = "macos") {
            return Err(RobotError::Unsupported(
                "the macOS robot is available only when the MCP server runs on macOS".into(),
            ));
        }
        Ok(())
    }

    async fn run(&self, program: &str, args: &[String]) -> Result<Vec<u8>, RobotError> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not run {program}: {error}"))
            })?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RobotError::CommandFailed(format!(
                "{program} exited with {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".into(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    async fn apple_script(&self, script: &str, values: &[String]) -> Result<(), RobotError> {
        let mut args = vec!["-e".into(), script.into(), "--".into()];
        args.extend(values.iter().cloned());
        self.run(&self.osascript_path, &args).await.map(|_| ())
    }

    async fn accessibility_elements(
        &self,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        #[cfg(target_os = "macos")]
        {
            let filter = filter.map(str::to_owned).map(|value| value.to_lowercase());
            let application = *self.active_process_id.lock().await;
            let window = self.active_window_id.lock().await.clone();
            tokio::time::timeout(
                Duration::from_millis(2_500),
                tokio::task::spawn_blocking(move || {
                    native_accessibility_elements(
                        filter.as_deref(),
                        max_results,
                        application,
                        window,
                    )
                }),
            )
            .await
            .map_err(|_| {
                RobotError::CommandFailed(
                    "macOS native accessibility filter exceeded its 2.5 second deadline".into(),
                )
            })?
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "macOS accessibility traversal task failed: {error}"
                ))
            })?
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (filter, max_results);
            Err(RobotError::Unsupported(
                "the macOS robot is available only when the MCP server runs on macOS".into(),
            ))
        }
    }

    async fn require_input_target(&self) -> Result<(), RobotError> {
        #[cfg(target_os = "macos")]
        if let Some(target) = *self.active_process_id.lock().await {
            let foreground = native_frontmost_application(None)?;
            let mut pid = 0;
            if unsafe { AXUIElementGetPid(foreground.0, &mut pid) } != 0 || pid != target {
                return Err(RobotError::InvalidInput(
                    "Activate the selected macOS app before typing, scrolling or dragging".into(),
                ));
            }
        }
        Ok(())
    }

    async fn accessibility_tap(&self, point: Point) -> Result<(), RobotError> {
        #[cfg(target_os = "macos")]
        {
            let application = *self.active_process_id.lock().await;
            if let Some(app) = self.active_application.lock().await.clone() {
                self.focus_command("activate", &app).await?;
                // AppKit may return before WindowServer publishes foreground
                // focus. AXPress can succeed on a background editor, so wait
                // before reporting the tap ready for a following keyboard step.
                wait_for_activation(|| self.require_input_target(), Duration::from_secs(2)).await?;
            }
            if native_accessibility_tap(point, application).is_ok() {
                return Ok(());
            }
            // Flutter can expose its widget tree through the bridge while the
            // native AX tree has no actionable editor. Deliver real pointer
            // input only after checking the selected foreground window.
            {
                let front = native_frontmost_application(None)?;
                let mut pid = 0;
                if unsafe { AXUIElementGetPid(front.0, &mut pid) } != 0
                    || application.is_some_and(|selected| selected != pid)
                {
                    return Err(RobotError::InvalidInput(
                        "Selected app is not foreground; input was not sent".into(),
                    ));
                }
                let attrs = NativeAxAttributes::new()?;
                let window =
                    native_required_attribute(front.0, attrs.focused_window, "AXFocusedWindow")?;
                if native_bounds_contains(window.0, &attrs, point) != Some(true) {
                    return Err(RobotError::InvalidInput(
                        "Input is outside the selected app window".into(),
                    ));
                }
            }
            pointer::drag(point, point, 1).await
        }
        #[cfg(not(target_os = "macos"))]
        {
            // AXPress targets the smallest actionable element under the coordinate.
            // It is more reliable than a synthetic desktop click for Chrome's tab
            // strip and address field, while preserving coordinate-based MCP input.
            self.apple_script(
            r#"on run argv
set targetX to item 2 of argv as real
set targetY to item 3 of argv as real
set targetApp to item 1 of argv
tell application "System Events"
    if targetApp is "" then
        set frontProcess to first application process whose frontmost is true
    else
        set frontProcess to application process targetApp
    end if
    tell frontProcess
        set bestCandidate to missing value
        set bestArea to 0
        try
            set candidates to entire contents of front window
        on error
            set candidates to {}
        end try
        repeat with candidate in candidates
            try
                set {x, y} to position of candidate
                set {w, h} to size of candidate
                set candidateRole to role of candidate as text
                set candidateArea to w * h
                if targetX ≥ x and targetX ≤ (x + w) and targetY ≥ y and targetY ≤ (y + h) and candidateRole is in {"AXButton", "AXLink", "AXTextField", "AXRadioButton", "AXPopUpButton", "AXMenuItem"} then
                    if bestArea is 0 or candidateArea < bestArea then
                        set bestCandidate to candidate
                        set bestArea to candidateArea
                    end if
                end if
            end try
        end repeat
        if bestCandidate is not missing value then
            try
                perform action "AXPress" of bestCandidate
            on error
                click bestCandidate
            end try
        else
            click at {targetX, targetY}
        end if
    end tell
end tell
end run"#,
            &[
                self.active_application.lock().await.clone().unwrap_or_default(),
                point.x.round().to_string(),
                point.y.round().to_string(),
            ],
        )
        .await
        }
    }
}

#[cfg(test)]
fn parse_accessibility_elements(output: &str) -> Vec<UiElement> {
    output
        .trim()
        .split('\x1e')
        .filter_map(|line| {
            let fields = line.split('\x1f').collect::<Vec<_>>();
            if fields.len() != 8 {
                return None;
            }
            let x = fields[4].trim().parse::<f64>().ok()?;
            let y = fields[5].trim().parse::<f64>().ok()?;
            let width = fields[6].trim().parse::<f64>().ok()?;
            let height = fields[7].trim().parse::<f64>().ok()?;
            let role = fields[0].trim();
            let title = non_empty(fields[1]);
            let description = non_empty(fields[2]);
            let value = non_empty(fields[3]);
            Some(UiElement {
                class_name: if role.is_empty() {
                    "AXUnknown".into()
                } else {
                    role.into()
                },
                resource_id: None,
                text: title.or_else(|| value.clone()),
                content_description: description,
                bounds: robot_core::Bounds {
                    left: x.round() as i32,
                    top: y.round() as i32,
                    right: (x + width).round() as i32,
                    bottom: (y + height).round() as i32,
                },
                clickable: matches!(role, "AXButton" | "AXLink" | "AXMenuItem" | "AXPopUpButton"),
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value,
                element_type: None,
                web_context: None,
            })
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.into())
}

#[async_trait]
impl Robot for MacosRobot {
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
        Some(Arc::new(session))
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
        self.require_device(device_id)?;
        self.capture_desktop(&robot_core::ScreenshotOptions::default())
            .await
    }

    async fn take_screenshot_with_options(
        &self,
        device_id: &str,
        options: &robot_core::ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
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
        let path = self.artifact_dir.join(format!(
            "macos-recording-{}.mp4",
            RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // Select AVFoundation's named desktop source, never its numeric device
        // index: indexes can point at a connected Continuity camera. Unlike
        // `screencapture -v`, ffmpeg finalizes a valid MP4 on SIGINT.
        let input = format!("{}:none", Self::screen_capture_device());
        let child = Command::new(&self.ffmpeg_path)
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
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start ffmpeg screen recording (run --install-chrome-recording-tools or set DEVICE_BOT_FFMPEG_PATH): {error}"
                ))
            })?;
        *recording = Some(ActiveRecording {
            path: path.clone(),
            child,
        });
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        let Some(recording) = self.recording.lock().await.take() else {
            return Err(RobotError::InvalidInput(
                "no macOS screen recording is in progress".into(),
            ));
        };
        #[cfg(target_os = "macos")]
        if let Some(pid) = recording.child.id() {
            // ffmpeg finalizes a playable MP4 when interrupted with SIGINT.
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let recording = {
            let mut recording = recording;
            recording.child.start_kill().map_err(|error| {
                RobotError::CommandFailed(format!("could not stop screen recording: {error}"))
            })?;
            recording
        };
        let output = recording.child.wait_with_output().await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not finalize macOS screen recording: {error}"
            ))
        })?;
        if !recording.path.is_file() {
            return Err(RobotError::CommandFailed(format!(
                "macOS did not create a screen-recording artifact (ffmpeg status {}): {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _guard = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            let target = self.live_guard(device_id, &[point]).await?;
            #[cfg(target_os = "macos")]
            return native_accessibility_tap(point, Some(target.pid));
            #[cfg(not(target_os = "macos"))]
            let _ = target;
        }
        self.accessibility_tap(point).await
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
        if live::Target::parse(device_id)?.is_some() {
            // The widget bridge identifies only a PID, not an individual window.
            return Ok(vec![]);
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

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return self.live_elements(device_id, None, None).await;
        }
        self.accessibility_elements(None, None).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return self.live_elements(device_id, filter, max_results).await;
        }
        // Filter during native AXUIElement traversal in the MCP process, and
        // stop descending once the requested result count has been reached.
        self.accessibility_elements(filter, max_results).await
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
mod screenshot_tests;

mod screenshot;

mod pointer;

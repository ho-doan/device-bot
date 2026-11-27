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

#[cfg(test)]
use robot_core::Robot;
use robot_core::{ElementQuery, Point, RobotError, UiElement};
use serde_json::{Value, json};
use tokio::{process::Command, sync::Mutex};

#[cfg(target_os = "macos")]
mod accessibility;
mod adapter;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod fixture;
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

mod recording;
use recording::ActiveRecording;

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

#[cfg(target_os = "macos")]
async fn capability_ffmpeg_listing(path: &str, argument: &str) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let mut child = Command::new(path)
        .args(["-hide_banner", argument])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        stdout.take(1_048_577).read_to_end(&mut bytes).await.ok()?;
        if bytes.len() > 1_048_576 {
            return None;
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        String::from_utf8(bytes).ok()
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
    }
    result
}

#[cfg(any(target_os = "macos", test))]
fn mac_ax_readiness(trusted: bool, query_ok: bool) -> robot_core::Capability {
    if !trusted {
        robot_core::Capability::blocked(
            "Grant Accessibility permission to the server process in System Settings",
        )
    } else if !query_ok {
        robot_core::Capability::blocked(
            "Select an accessible foreground app on an interactive desktop",
        )
    } else {
        robot_core::Capability::available(
            "Accessibility permission and read-only target query succeeded; later input can still fail",
        )
    }
}

#[cfg(any(target_os = "macos", test))]
fn mac_display_readiness(permitted: bool, display_ready: bool) -> robot_core::Capability {
    if !permitted {
        robot_core::Capability::blocked(
            "Grant Screen Recording permission to the server process in System Settings",
        )
    } else if !display_ready {
        robot_core::Capability::blocked("Connect an active display to the macOS desktop session")
    } else {
        robot_core::Capability::available(
            "Screen Recording permission and active display are present; capture can still fail",
        )
    }
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
    #[cfg(target_os = "macos")]
    flutter_windows: Arc<Mutex<std::collections::HashSet<String>>>,
    #[cfg(target_os = "macos")]
    fixture: Arc<Mutex<Option<fixture::Binding>>>,
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
            #[cfg(target_os = "macos")]
            flutter_windows: Arc::default(),
            #[cfg(target_os = "macos")]
            fixture: Arc::default(),
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
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        #[cfg(target_os = "macos")]
        {
            let application = *self.active_process_id.lock().await;
            let window = self.active_window_id.lock().await.clone();
            tokio::time::timeout(
                Duration::from_millis(2_500),
                tokio::task::spawn_blocking(move || {
                    native_accessibility_elements(&query, application, window)
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
            let _ = query;
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

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "tests/screenshot.rs"]
mod screenshot_tests;

mod screenshot;

mod pointer;

#[cfg(test)]
#[path = "tests/recording.rs"]
mod recorder_ownership_regression;

#[cfg(test)]
#[path = "tests/capability_readiness.rs"]
mod capability_readiness_tests;

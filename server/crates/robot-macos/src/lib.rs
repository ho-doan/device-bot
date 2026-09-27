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

use async_trait::async_trait;
use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

const DEVICE_ID: &str = "macos-host";
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

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
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("MOBILE_MCP_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()))
    }

    fn ffmpeg_path() -> String {
        std::env::var("MOBILE_MCP_FFMPEG_PATH").unwrap_or_else(|_| {
            [
                "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
                "/usr/local/opt/ffmpeg/bin/ffmpeg",
            ]
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
            .map_or_else(|| "ffmpeg".into(), |path| (*path).into())
        })
    }

    fn require_device(&self, device_id: &str) -> Result<(), RobotError> {
        if device_id != DEVICE_ID {
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
}

#[async_trait]
impl Robot for MacosRobot {
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

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create screenshot directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "macos-screenshot-{}.png",
            SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        self.run(
            &self.screencapture_path,
            &["-x".into(), path.to_string_lossy().into_owned()],
        )
        .await?;
        let image = tokio::fs::read(&path).await.map_err(|error| {
            RobotError::CommandFailed(format!("could not read macOS screenshot: {error}"))
        });
        let _ = tokio::fs::remove_file(path).await;
        image
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
        // AVFoundation's desktop capture source is `1:none`. Unlike
        // `screencapture -v`, ffmpeg finalizes a valid MP4 on SIGINT.
        let child = Command::new(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "avfoundation",
                "-capture_cursor",
                "1",
                "-framerate",
                "30",
                "-i",
                "1:none",
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
                    "could not start ffmpeg screen recording (run --install-chrome-recording-tools or set MOBILE_MCP_FFMPEG_PATH): {error}"
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
        if let Some(pid) = recording.child.id() {
            // ffmpeg finalizes a playable MP4 when interrupted with SIGINT.
            #[cfg(target_os = "macos")]
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
            }
            #[cfg(not(target_os = "macos"))]
            recording.child.start_kill().map_err(|error| {
                RobotError::CommandFailed(format!("could not stop screen recording: {error}"))
            })?;
        }
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
        self.apple_script(
            "on run argv\n tell application \"System Events\" to click at {(item 1 of argv as integer), (item 2 of argv as integer)}\nend run",
            &[point.x.round().to_string(), point.y.round().to_string()],
        ).await
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.run(&self.open_path, &["-a".into(), app_id.into()])
            .await
            .map(|_| ())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let mut args = vec![];
        if let Some(app_id) = app_id {
            args.extend(["-a".into(), app_id.into()]);
        }
        args.push(url.into());
        self.run(&self.open_path, &args).await.map(|_| ())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.apple_script(
            "on run argv\n tell application \"System Events\" to keystroke (item 1 of argv)\nend run",
            &[text.into()],
        ).await
    }

    async fn swipe(
        &self,
        device_id: &str,
        _start: Point,
        _end: Point,
        _duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "macOS drag gestures require a pointer automation backend; tap and keyboard input are available through System Events".into(),
        ))
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

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "accessibility-tree discovery is not supported by the macOS robot".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_device_ids() {
        assert!(
            MacosRobot::new("artifacts")
                .require_device("another-device")
                .is_err()
        );
    }
}

//! Android implementation of the platform-neutral [`Robot`] contract.
//!
//! Process execution is isolated behind [`CommandRunner`], letting the ADB
//! parser and command construction be tested without an emulator or `adb`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use robot_core::{Bounds, ElementType, ImeAction, Point, Robot};
use robot_core::{DeviceState, Platform, RobotDevice, RobotError};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use url::Url;

mod adapter;
mod command;
mod protocol;

use command::helper_command;
pub use command::{CommandOutput, CommandRunner, TokioCommandRunner};
pub use protocol::{
    parse_adb_devices, parse_adb_installed_apps, parse_focused_ime_action, parse_uiautomator_xml,
};
mod emulator;
mod video;

fn android_probe_readiness(ok: bool, ready: &str, blocked: &str) -> robot_core::Capability {
    if ok {
        robot_core::Capability::available(format!("{ready}; later operations can fail"))
    } else {
        robot_core::Capability::blocked(blocked)
    }
}

static TREE_DUMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct ActiveRecording {
    device_path: String,
    artifact_path: PathBuf,
    pid: u32,
}

/// ADB-backed Android device driver.
#[derive(Debug, Clone)]
pub struct AndroidRobot<R = TokioCommandRunner> {
    adb_path: String,
    emulator_path: String,
    boot_lock: Arc<Mutex<()>>,
    artifact_dir: PathBuf,
    recordings: Arc<Mutex<HashMap<String, ActiveRecording>>>,
    tree_dump_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    runner: R,
    video_controls: Arc<video::Controls>,
}

impl AndroidRobot<TokioCommandRunner> {
    pub fn new(adb_path: impl Into<String>, artifact_dir: impl Into<PathBuf>) -> Self {
        Self::with_runner_and_artifact_dir(adb_path, artifact_dir, TokioCommandRunner)
    }
}

impl<R> AndroidRobot<R> {
    pub fn with_runner(adb_path: impl Into<String>, runner: R) -> Self {
        Self::with_runner_and_artifact_dir(adb_path, "artifacts", runner)
    }

    pub fn with_runner_and_artifact_dir(
        adb_path: impl Into<String>,
        artifact_dir: impl Into<PathBuf>,
        runner: R,
    ) -> Self {
        let adb_path = adb_path.into();
        Self {
            emulator_path: emulator::emulator_path(&adb_path),
            adb_path,
            boot_lock: Arc::default(),
            artifact_dir: artifact_dir.into(),
            recordings: Arc::default(),
            tree_dump_locks: Arc::default(),
            runner,
            video_controls: Arc::default(),
        }
    }

    pub fn adb_path(&self) -> &str {
        &self.adb_path
    }

    pub fn with_emulator_path(mut self, path: impl Into<String>) -> Self {
        self.emulator_path = path.into();
        self
    }

    fn next_recording(&self, device_id: &str) -> ActiveRecording {
        let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let safe_device_id = device_id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        ActiveRecording {
            device_path: format!("/sdcard/mobile-mcp-{sequence}.mp4"),
            artifact_path: self.artifact_dir.join(format!(
                "mobile-mcp-android-recording-{safe_device_id}-{sequence}.mp4"
            )),
            pid: 0,
        }
    }
}

impl<R: CommandRunner> AndroidRobot<R> {
    async fn run_adb(&self, args: &[String]) -> Result<CommandOutput, RobotError> {
        let output = self.runner.run(&self.adb_path, args).await?;
        if output.success {
            return Ok(output);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let suffix = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        Err(RobotError::CommandFailed(format!(
            "adb exited with status {}{}",
            output
                .exit_code
                .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
            suffix
        )))
    }
}

fn ensure_device_id(device_id: &str) -> Result<(), RobotError> {
    if device_id.starts_with("avd:") {
        return Err(RobotError::InvalidInput(
            "start the AVD first, refresh devices, then use its emulator-* ADB id".into(),
        ));
    }
    if device_id.trim().is_empty() {
        return Err(RobotError::InvalidInput(
            "device id must not be empty".into(),
        ));
    }
    Ok(())
}

fn ensure_app_id(app_id: &str) -> Result<(), RobotError> {
    let valid = app_id.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    }) && app_id.contains('.');
    if valid {
        Ok(())
    } else {
        Err(RobotError::InvalidInput(
            "app id must be a dot-separated Android package name".into(),
        ))
    }
}

fn ensure_web_url(value: &str) -> Result<(), RobotError> {
    let url = Url::parse(value)
        .map_err(|error| RobotError::InvalidInput(format!("invalid URL: {error}")))?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(RobotError::InvalidInput(
            "URL scheme must be http or https".into(),
        ))
    }
}

fn encode_input_text(text: &str) -> Result<String, RobotError> {
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(RobotError::InvalidInput(
            "text must be non-empty and contain no control characters".into(),
        ));
    }
    Ok(text.replace(' ', "%s"))
}

fn ensure_apk_path(apk_path: &str) -> Result<(), RobotError> {
    if apk_path.trim().is_empty() || !apk_path.ends_with(".apk") {
        return Err(RobotError::InvalidInput(
            "apk path must name a non-empty .apk file".into(),
        ));
    }
    Ok(())
}

/// Parses an Android `uiautomator dump` XML document into portable elements.
fn android_coordinate(value: f64, axis: &str) -> Result<i32, RobotError> {
    if !value.is_finite() || !(i32::MIN as f64..=i32::MAX as f64).contains(&value) {
        return Err(RobotError::InvalidInput(format!(
            "{axis} must be a finite 32-bit coordinate"
        )));
    }
    Ok(value.round() as i32)
}

/// Parses the output from `adb devices -l`.
#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests/capability_probe.rs"]
mod capability_probe_tests;

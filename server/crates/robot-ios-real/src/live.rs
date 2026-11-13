//! Event-driven USB iOS screen video. Selection is by capture unique ID, never camera index/name.
use robot_core::{RobotError, VideoStream};
use serde::Deserialize;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

const HELPER_SOURCE: &str = include_str!("ios_live.swift");
static COMPILE_GATE: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

fn failure(message: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("iOS Live: {message}"))
}

#[derive(Clone, Debug)]
pub(crate) struct Source {
    helper: PathBuf,
    capture_id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
struct CaptureDevice {
    id: String,
}

fn normalized(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

/// Reject incomplete or letterboxed capture geometry before enabling input.
pub(crate) fn validate_geometry(
    logical_width: f64,
    logical_height: f64,
    physical_width: u32,
    physical_height: u32,
) -> Result<(), RobotError> {
    if !logical_width.is_finite()
        || !logical_height.is_finite()
        || logical_width <= 0.0
        || logical_height <= 0.0
        || physical_width < 2
        || physical_height < 2
    {
        return Err(failure("Invalid capture or XCTest screen dimensions"));
    }
    let scale_x = physical_width as f64 / logical_width;
    let scale_y = physical_height as f64 / logical_height;
    // Native pixel dimensions can round by a pixel; larger differences mean
    // that the video does not cover the same logical screen used by XCTest.
    if (scale_x / scale_y - 1.0).abs() > 0.02 {
        return Err(failure(
            "iOS capture and XCTest screen geometry differ; reopen Live after rotation completes",
        ));
    }
    Ok(())
}

fn select_source(
    device_id: &str,
    devices: &[CaptureDevice],
    mappings: &HashMap<String, String>,
) -> Result<String, RobotError> {
    let requested = mappings.get(device_id);
    let matching: Vec<_> = devices
        .iter()
        .filter(|device| match requested {
            Some(exact) => device.id == *exact,
            None => normalized(&device.id) == normalized(device_id),
        })
        .collect();
    match matching.as_slice() {
        [device] => Ok(device.id.clone()),
        _ => Err(failure(
            "No unique USB screen source matches the selected iPhone UDID. Connect and trust the exact phone over USB. If AVFoundation uses a different ID, explicitly map its --list ID with DEVICE_BOT_IOS_CAPTURE_MAP (JSON device-UDID to capture-ID); another camera/phone is never selected automatically.",
        )),
    }
}

async fn helper_path(artifact_dir: &Path, xcrun: &str) -> Result<PathBuf, RobotError> {
    if let Some(override_path) = std::env::var_os("DEVICE_BOT_IOS_LIVE_HELPER") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(failure(
            "DEVICE_BOT_IOS_LIVE_HELPER does not name an existing executable",
        ));
    }
    let _guard = COMPILE_GATE.lock().await;
    let directory = artifact_dir.join("native-helpers");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(failure)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    HELPER_SOURCE.hash(&mut hash);
    let filename = format!("ios-live-{:x}", hash.finish());
    let executable = directory.join(&filename);
    if executable.is_file() {
        return Ok(executable);
    }
    let source = directory.join(format!("{filename}.swift"));
    tokio::fs::write(&source, HELPER_SOURCE)
        .await
        .map_err(failure)?;
    let pending = directory.join(format!("{filename}.pending-{}", std::process::id()));
    let output = tokio::time::timeout(
        Duration::from_secs(90),
        Command::new(xcrun)
            .arg("swiftc")
            .arg(&source)
            .arg("-o")
            .arg(&pending)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(failure)?
    .map_err(failure)?;
    if !output.status.success() {
        return Err(failure(format!(
            "The native capture helper needs Xcode Command Line Tools (xcrun swiftc). Compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    tokio::fs::rename(&pending, &executable)
        .await
        .map_err(failure)?;
    Ok(executable)
}

async fn query(helper: &Path, args: &[&str]) -> Result<Vec<u8>, RobotError> {
    let output = tokio::time::timeout(Duration::from_secs(15), Command::new(helper)
        .args(args).stdin(Stdio::null()).kill_on_drop(true).output()).await
        .map_err(|_| failure("USB screen capture did not produce a frame within 15 seconds; unlock/trust the selected phone and retry"))?
        .map_err(failure)?;
    if !output.status.success() {
        return Err(failure(String::from_utf8_lossy(&output.stderr)));
    }
    Ok(output.stdout)
}

pub(crate) async fn prepare(
    artifact_dir: &Path,
    xcrun: &str,
    device_id: &str,
) -> Result<Source, RobotError> {
    if !cfg!(target_os = "macos") {
        return Err(RobotError::Unsupported(
            "Real-iOS Live requires a Mac with a USB-connected iPhone".into(),
        ));
    }
    let helper = helper_path(artifact_dir, xcrun).await?;
    let devices: Vec<CaptureDevice> =
        serde_json::from_slice(&query(&helper, &["--list"]).await?).map_err(failure)?;
    let mappings = std::env::var("DEVICE_BOT_IOS_CAPTURE_MAP")
        .ok()
        .map(|json| serde_json::from_str::<HashMap<String, String>>(&json))
        .transpose()
        .map_err(failure)?
        .unwrap_or_default();
    let capture_id = select_source(device_id, &devices, &mappings)?;
    #[derive(Deserialize)]
    struct Dimensions {
        width: u32,
        height: u32,
    }
    let dimensions: Dimensions =
        serde_json::from_slice(&query(&helper, &["--probe", &capture_id]).await?)
            .map_err(failure)?;
    if dimensions.width < 2
        || dimensions.height < 2
        || dimensions.width > 16384
        || dimensions.height > 16384
    {
        return Err(failure("Invalid native screen dimensions"));
    }
    Ok(Source {
        helper,
        capture_id,
        width: dimensions.width,
        height: dimensions.height,
    })
}

pub(crate) async fn start(source: Source) -> Result<VideoStream, RobotError> {
    let mut command = Command::new(&source.helper);
    command
        .arg("--capture")
        .arg(&source.capture_id)
        .arg(source.width.to_string())
        .arg(source.height.to_string());
    robot_core::video::start_h264_process(command, source.width, source.height).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device(id: &str) -> CaptureDevice {
        CaptureDevice { id: id.into() }
    }
    #[test]
    fn geometry_requires_matching_screen_in_both_orientations() {
        assert!(validate_geometry(393.0, 852.0, 1179, 2556).is_ok());
        assert!(validate_geometry(852.0, 393.0, 2556, 1179).is_ok());
        assert!(validate_geometry(393.0, 852.0, 2556, 1179).is_err());
        assert!(validate_geometry(393.0, 852.0, 1179, 2400).is_err());
        assert!(validate_geometry(f64::NAN, 852.0, 1179, 2556).is_err());
        assert!(validate_geometry(393.0, 0.0, 1179, 2556).is_err());
        assert!(validate_geometry(393.0, 852.0, 0, 2556).is_err());
    }
    #[test]
    fn selection_never_falls_back_to_another_phone_or_camera() {
        let devices = [
            device("00008130000959E43EDA001C"),
            device("FaceTime Camera"),
        ];
        assert_eq!(
            select_source("00008130-000959E43EDA001C", &devices, &HashMap::new()).unwrap(),
            devices[0].id
        );
        assert!(select_source("another-phone", &devices, &HashMap::new()).is_err());
        assert!(
            select_source(
                "00008130-000959E43EDA001C",
                &[device("same-name-phone")],
                &HashMap::new()
            )
            .is_err()
        );
        assert!(select_source("abc", &[device("abc"), device("a-b-c")], &HashMap::new()).is_err());
    }
    #[test]
    fn explicit_mapping_requires_exact_unique_capture_id() {
        let mapping = HashMap::from([("phone-udid".into(), "external-screen-42".into())]);
        assert_eq!(
            select_source("phone-udid", &[device("external-screen-42")], &mapping).unwrap(),
            "external-screen-42"
        );
        assert!(select_source("other-udid", &[device("external-screen-42")], &mapping).is_err());
        assert!(select_source("phone-udid", &[device("external screen 42")], &mapping).is_err());
    }
}

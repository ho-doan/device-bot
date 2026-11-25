//! Event-driven USB iOS screen video. Selection is by capture unique ID, never camera index/name.
use robot_core::{RobotError, VideoStream};
use serde::Deserialize;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

const HELPER_SOURCE: &str = include_str!("ios_live.swift");
static COMPILE_GATE: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

fn failure(message: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("iOS Live: {message}"))
}

#[derive(Clone)]
pub(crate) struct Source {
    pending: Arc<Mutex<Option<VideoStream>>>,
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

fn select_source(device_id: &str, devices: &[CaptureDevice]) -> Result<Option<String>, RobotError> {
    let requested = normalized(device_id);
    let mut matches = devices
        .iter()
        .filter(|device| normalized(&device.id) == requested);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(failure(
            "Multiple USB screen sources match the selected iPhone UDID. Set DEVICE_BOT_IOS_CAPTURE_MAP to its exact AVFoundation ID.",
        ));
    }
    Ok(first.map(|device| device.id.clone()))
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
    let output = tokio::time::timeout(Duration::from_secs(20), Command::new(helper)
        .args(args).stdin(Stdio::null()).kill_on_drop(true).output()).await
        .map_err(|_| failure("USB screen discovery did not finish within 20 seconds; unlock/trust the selected phone and retry"))?
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
    try_prepare(artifact_dir, xcrun, device_id).await?.ok_or_else(|| failure(
        "No USB screen source matches the selected iPhone UDID. Unlock and trust the phone. If AVFoundation uses a different ID, map its --list ID with DEVICE_BOT_IOS_CAPTURE_MAP, or use DEVICE_BOT_IOS_LIVE_TRANSPORT=wifi."
    ))
}

/// Auto may fall back only when this phone has no USB source. Encoder,
/// permission, explicit mapping and ambiguous-source failures stay errors.
pub(crate) async fn try_prepare(
    artifact_dir: &Path,
    xcrun: &str,
    device_id: &str,
) -> Result<Option<Source>, RobotError> {
    if !cfg!(target_os = "macos") {
        return Err(RobotError::Unsupported(
            "Real-iOS Live requires a Mac with a USB-connected iPhone".into(),
        ));
    }
    let helper = helper_path(artifact_dir, xcrun).await?;
    let mappings = std::env::var("DEVICE_BOT_IOS_CAPTURE_MAP")
        .ok()
        .map(|json| serde_json::from_str::<HashMap<String, String>>(&json))
        .transpose()
        .map_err(failure)?
        .unwrap_or_default();
    prepare_source(
        helper,
        device_id,
        mappings.get(device_id).map(String::as_str),
    )
    .await
}

async fn prepare_source(
    helper: PathBuf,
    device_id: &str,
    mapped_id: Option<&str>,
) -> Result<Option<Source>, RobotError> {
    let capture_id = if let Some(mapped_id) = mapped_id {
        // --stream checks the exact ID in the same process that opens it. A
        // separate inventory process can miss an asynchronously registered source.
        mapped_id.to_owned()
    } else {
        let devices: Vec<CaptureDevice> =
            serde_json::from_slice(&query(&helper, &["--list"]).await?).map_err(failure)?;
        let Some(capture_id) = select_source(device_id, &devices)? else {
            return Ok(None);
        };
        capture_id
    };
    let mut stream = None;
    // After a short capture macOS can throttle its screen-capture daemon restart
    // for up to ten seconds. A discovery session created too early may remain
    // empty even after the daemon starts, so retry only absence in a fresh helper.
    for attempt in 0..3 {
        if attempt != 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let mut command = Command::new(&helper);
        command.args(["--stream", &capture_id]);
        stream = robot_core::video::start_h264_process_with_dimensions(command).await?;
        if stream.is_some() {
            break;
        }
    }
    let stream = stream.ok_or_else(|| {
        failure("The exact USB iOS screen source is unavailable; unlock, reconnect and trust the selected iPhone")
    })?;
    let (width, height) = (stream.width, stream.height);
    let pending = Arc::new(Mutex::new(Some(stream)));
    let unused = Arc::downgrade(&pending);
    tokio::spawn(async move {
        // The caller may abandon negotiation after preparing this source.
        tokio::time::sleep(Duration::from_secs(30)).await;
        if let Some(pending) = unused.upgrade() {
            pending.lock().await.take();
        }
    });
    Ok(Some(Source {
        pending,
        width,
        height,
    }))
}

pub(crate) async fn start(source: Source) -> Result<VideoStream, RobotError> {
    source
        .pending
        .lock()
        .await
        .take()
        .ok_or_else(|| failure("USB source was already started or expired; reopen Live"))
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
            select_source("00008130-000959E43EDA001C", &devices)
                .unwrap()
                .as_deref(),
            Some(devices[0].id.as_str())
        );
        assert!(select_source("another-phone", &devices).unwrap().is_none());
        assert!(
            select_source("00008130-000959E43EDA001C", &[device("same-name-phone")])
                .unwrap()
                .is_none()
        );
        assert!(select_source("abc", &[device("abc"), device("a-b-c")]).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn explicit_mapping_opens_exact_id_once_for_dimensions_and_frames() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        tokio::fs::create_dir(&directory).await.unwrap();
        let helper = directory.join("helper");
        tokio::fs::write(
            &helper,
            concat!(
                "#!/bin/sh\n[ \"$1:$2\" = '--stream:external-screen-42' ] || exit 1\n",
                "if [ ! -f \"$0.attempt\" ]; then touch \"$0.attempt\"; printf '\\000\\000\\000\\000\\000\\000\\000\\000'; exit; fi\n",
                "printf '\\000\\000\\005\\012\\000\\000\\012\\354\\000\\000\\000\\001\\011\\360\\000\\000\\000\\001\\145\\001\\000\\000\\000\\001\\011\\360'\n",
            ),
        )
        .await
        .unwrap();
        tokio::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        let valid = prepare_source(helper.clone(), "phone-udid", Some("external-screen-42")).await;
        let missing = prepare_source(helper, "phone-udid", Some("external screen 42")).await;
        tokio::fs::remove_dir_all(directory).await.unwrap();

        let source = valid.unwrap().unwrap();
        assert_eq!((source.width, source.height), (1290, 2796));
        // Taking the live source needs neither the helper executable nor a second
        // connection to the phone. The first frame survives the handoff.
        let mut stream = start(source.clone()).await.unwrap();
        assert!(stream.frames.recv().await.unwrap().unwrap().keyframe);
        assert!(start(source).await.is_err());
        assert!(missing.is_err());
    }
}

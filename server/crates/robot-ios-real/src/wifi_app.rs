//! Build/install only the signed ReplayKit app, independent of the XCTest helper.
use crate::wifi_video::BroadcastConfiguration;
use robot_core::RobotError;
use std::{path::Path, process::Stdio, sync::LazyLock, time::Duration};
use tokio::{process::Command, sync::Mutex};

static BUILD_GATE: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

async fn run(
    mut command: Command,
    label: &str,
    seconds: u64,
    log: &Path,
) -> Result<(), RobotError> {
    let output = tokio::time::timeout(
        Duration::from_secs(seconds),
        command.stdin(Stdio::null()).kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| {
        RobotError::CommandFailed(format!(
            "iOS Wi-Fi {label} timed out; see {}",
            log.display()
        ))
    })?
    .map_err(|error| RobotError::CommandFailed(format!("iOS Wi-Fi {label}: {error}")))?;
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    let _ = tokio::fs::write(log, &bytes).await;
    if !output.status.success() {
        let tail = &bytes[bytes.len().saturating_sub(4000)..];
        return Err(RobotError::CommandFailed(format!(
            "iOS Wi-Fi {label} failed; see {}: {}",
            log.display(),
            String::from_utf8_lossy(tail)
        )));
    }
    Ok(())
}

pub(crate) async fn launch(
    runner_project: &Path,
    xcodebuild: &str,
    xcrun: &str,
    artifacts: &Path,
    device_id: &str,
    configuration: impl FnOnce() -> BroadcastConfiguration + Send,
) -> Result<(), RobotError> {
    let _build = BUILD_GATE.lock().await;
    let root = runner_project
        .parent()
        .ok_or_else(|| RobotError::InvalidInput("Missing iOS runner assets".into()))?;
    let project = root.join("WifiBroadcast.xcodeproj");
    if !project.is_dir() {
        return Err(RobotError::Unsupported("The iOS runner asset bundle is missing WifiBroadcast.xcodeproj; install an updated Device Bot build".into()));
    }
    let team = crate::configure_xctest_signing(&project, None, None)
        .await?
        .ok_or_else(|| {
            RobotError::CommandFailed(
                "iOS Wi-Fi needs the runner signing setup script and an Apple Development identity"
                    .into(),
            )
        })?;
    tokio::fs::create_dir_all(artifacts)
        .await
        .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
    let derived = root.join("build/wifi-broadcast");
    let mut build = Command::new(xcodebuild);
    build
        .args(["build", "-project"])
        .arg(&project)
        .args([
            "-scheme",
            "DeviceBotBroadcast",
            "-configuration",
            "Debug",
            "-destination",
            "generic/platform=iOS",
            "-derivedDataPath",
        ])
        .arg(&derived)
        .arg("-allowProvisioningUpdates")
        .env_remove("SDKROOT");
    run(
        build,
        "broadcast build/signing",
        240,
        &artifacts.join("ios-wifi-build.log"),
    )
    .await?;
    let mut install = Command::new(xcrun);
    install
        .args([
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            device_id,
        ])
        .arg(derived.join("Build/Products/Debug-iphoneos/DeviceBotBroadcast.app"));
    run(
        install,
        "broadcast install",
        60,
        &artifacts.join("ios-wifi-install.log"),
    )
    .await?;
    let mut launch = Command::new(xcrun);
    launch
        .args([
            "devicectl",
            "device",
            "process",
            "launch",
            "--terminate-existing",
            "--device",
            device_id,
        ])
        .arg(format!("com.devicebot.wifi.{team}"))
        // devicectl forwards prefixed environment variables to the app. Keep
        // upload credentials out of process arguments and diagnostic strings.
        .env(
            "DEVICECTL_CHILD_DEVICE_BOT_WIFI_CONFIG",
            serde_json::to_string(&configuration())
                .map_err(|e| RobotError::InvalidInput(e.to_string()))?,
        );
    run(
        launch,
        "broadcast launch",
        30,
        &artifacts.join("ios-wifi-launch.log"),
    )
    .await
}

use std::{
    collections::HashMap, fs::File, net::IpAddr, path::PathBuf, process::Stdio, sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use robot_core::{
    Bounds, DeviceState, ElementQuery, ImeAction, Platform, Point, Robot, RobotDevice, RobotError,
    UiAction, UiElement, UiRunnerStatus, apply_element_query,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::sleep,
};
use url::Url;
use uuid::Uuid;

use crate::{
    quicktime_recording::QuickTimeRecording,
    runner_cache::{RunnerCache, compatible, live_status},
    xctest_agent::XCTestAgentBridge,
};

const ACTION_TIMEOUT: Duration = Duration::from_secs(45);
const AGENT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const AGENT_START_TIMEOUT: Duration = Duration::from_secs(60);

/// Launches the signed real-device XCTest command loop from the core driver.
/// Keeping this in the driver makes a fresh MCP process reproducible: it no
/// longer depends on an unrelated terminal that happened to start the agent.
#[derive(Clone, Debug)]
pub struct XCTestAgentLauncher {
    xcodebuild_path: String,
    project: PathBuf,
    derived_data: PathBuf,
    hosts: String,
    port: u16,
    token: Option<String>,
}

impl XCTestAgentLauncher {
    pub fn new(
        xcodebuild_path: impl Into<String>,
        project: impl Into<PathBuf>,
        derived_data: impl Into<PathBuf>,
        hosts: impl Into<String>,
        port: u16,
        token: Option<String>,
    ) -> Result<Self, RobotError> {
        let raw_hosts = hosts.into();
        let hosts = raw_hosts
            .split(',')
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .collect::<Vec<_>>();
        if hosts.is_empty()
            || hosts
                .iter()
                .any(|host| !callback_host_is_advertisable(host))
        {
            return Err(RobotError::InvalidInput(
                "DEVICE_BOT_IOS_AGENT_HOSTS must be a comma-separated list of LAN IPs or hostnames the iPhone can open; loopback, wildcard, and link-local addresses are not reachable from the device"
                    .into(),
            ));
        }
        Ok(Self {
            xcodebuild_path: xcodebuild_path.into(),
            project: project.into(),
            derived_data: derived_data.into(),
            hosts: hosts.join(","),
            port,
            token: token.filter(|token| !token.is_empty()),
        })
    }

    fn cache(&self) -> RunnerCache<'_> {
        RunnerCache {
            project: &self.project,
            derived_data: &self.derived_data,
            simulator: false,
        }
    }

    async fn ensure_build(&self) -> Result<(), RobotError> {
        let cache = self.cache();
        let version = cache.version()?;
        if cache.is_current(&version) {
            return Ok(());
        }
        let output = tokio::time::timeout(
            Duration::from_secs(180),
            Command::new(&self.xcodebuild_path)
                .args(["build-for-testing", "-allowProvisioningUpdates", "-project"])
                .arg(&self.project)
                .args([
                    "-scheme",
                    "MobileMcpRealRunner",
                    "-destination",
                    "generic/platform=iOS",
                    "-derivedDataPath",
                ])
                .arg(&self.derived_data)
                .arg(format!("DEVICE_BOT_BUILD_ID={version}"))
                .env_remove("SDKROOT")
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| RobotError::CommandFailed("iOS XCTest build timed out after 180s".into()))?
        .map_err(|e| {
            RobotError::CommandFailed(format!("could not build iOS XCTest helper: {e}"))
        })?;
        if !output.status.success() {
            return Err(RobotError::CommandFailed(format!(
                "iOS XCTest build/signing failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        cache.mark_current(&version)
    }

    async fn start(&self, device_id: &str) -> Result<Child, RobotError> {
        validate_device_id(device_id)?;
        self.ensure_build().await?;
        let project = self.project.to_string_lossy().into_owned();
        let derived_data = self.derived_data.to_string_lossy().into_owned();
        let destination = format!("platform=iOS,id={device_id}");
        let mut command = Command::new(&self.xcodebuild_path);
        command
            .args([
                "test-without-building",
                "-project",
                &project,
                "-scheme",
                "MobileMcpRealRunner",
                "-destination",
                &destination,
                "-derivedDataPath",
                &derived_data,
                "-only-testing:MobileMcpRealRunnerUITests/MobileMcpRealRunnerUITests/testServe",
            ])
            .env_remove("SDKROOT")
            .stdin(Stdio::null())
            .env("TEST_RUNNER_DEVICE_BOT_DEVICE_ID", device_id);
        #[cfg(unix)]
        command.process_group(0);
        // Xcode strips TEST_RUNNER_ and injects the remainder into the
        // on-device XCTest process. The runner reads DEVICE_BOT_*.
        for (key, value) in self.test_environment() {
            command.env(key, value);
        }
        let log_path = self
            .derived_data
            .join("Logs/Agent")
            .join(format!("{device_id}-xcodebuild.log"));
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest agent log directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let stdout = File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not open XCTest agent log {}: {error}",
                    log_path.display()
                ))
            })?;
        let stderr = stdout.try_clone().map_err(|error| {
            RobotError::CommandFailed(format!("could not duplicate XCTest agent log: {error}"))
        })?;
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        command.spawn().map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not start the real-iOS XCTest agent: {error}"
            ))
        })
    }

    fn test_environment(&self) -> Vec<(String, String)> {
        let mut environment = vec![
            ("TEST_RUNNER_DEVICE_BOT_HOSTS".into(), self.hosts.clone()),
            ("TEST_RUNNER_DEVICE_BOT_PORT".into(), self.port.to_string()),
        ];
        if let Some(token) = &self.token {
            environment.push(("TEST_RUNNER_DEVICE_BOT_TOKEN".into(), token.clone()));
        }
        environment
    }
}

fn callback_host_is_advertisable(host: &str) -> bool {
    if host.is_empty()
        || host.contains("//")
        || host.contains('/')
        || host.chars().any(char::is_whitespace)
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_link_local()
                && !address.is_multicast()
        }
        Ok(IpAddr::V6(address)) => {
            !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_unicast_link_local()
                && !address.is_multicast()
        }
        Err(_) => true,
    }
}

pub struct IosXCTestRobot {
    xcrun_path: String,
    agent: XCTestAgentBridge,
    artifact_dir: PathBuf,
    recording: Arc<Mutex<Option<QuickTimeRecording>>>,
    agent_launcher: Option<XCTestAgentLauncher>,
    persistent_agents: Arc<Mutex<HashMap<String, Child>>>,
    runner_project: PathBuf,
}

impl IosXCTestRobot {
    pub fn new(xcrun_path: impl Into<String>, agent: XCTestAgentBridge) -> Self {
        Self::with_artifact_dir(xcrun_path, agent, "artifacts")
    }

    pub fn with_artifact_dir(
        xcrun_path: impl Into<String>,
        agent: XCTestAgentBridge,
        artifact_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            xcrun_path: xcrun_path.into(),
            agent,
            artifact_dir: artifact_dir.into(),
            recording: Arc::default(),
            agent_launcher: None,
            persistent_agents: Arc::default(),
            runner_project: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("xctest-runner/MobileMcpRealRunner.xcodeproj"),
        }
    }

    pub fn with_agent_launcher(mut self, launcher: XCTestAgentLauncher) -> Self {
        self.runner_project = launcher.project.clone();
        self.agent_launcher = Some(launcher);
        self
    }

    pub fn with_runner_project(mut self, project: PathBuf) -> Self {
        self.runner_project = project;
        self
    }

    async fn xcrun(&self, args: &[&str]) -> Result<String, RobotError> {
        let output = Command::new(&self.xcrun_path)
            .args(args)
            .output()
            .await
            .map_err(|error| RobotError::CommandFailed(format!("could not run xcrun: {error}")))?;
        if output.status.success() {
            return String::from_utf8(output.stdout).map_err(|error| {
                RobotError::CommandFailed(format!("xcrun returned invalid UTF-8: {error}"))
            });
        }
        Err(RobotError::CommandFailed(format!(
            "xcrun {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim(),
        )))
    }

    async fn ensure_agent_started(&self, device_id: &str) -> Result<(), RobotError> {
        let Some(launcher) = &self.agent_launcher else {
            return Ok(());
        };
        let version = launcher.cache().version()?;
        if let Ok(health) = self
            .agent
            .dispatch_for(device_id, "health", json!({}), AGENT_PROBE_TIMEOUT)
            .await
        {
            if compatible(&health, device_id, &version) {
                return Ok(());
            }
            let _ = self
                .agent
                .dispatch_for(device_id, "stop", json!({}), AGENT_PROBE_TIMEOUT)
                .await;
            if let Some(mut child) = self.persistent_agents.lock().await.remove(device_id) {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        // Synchronize signing before calculating the build identity: changing the
        // project team must invalidate cached products and the helper version.
        crate::configure_xctest_signing(&launcher.project, None, None).await?;
        let version = launcher.cache().version()?;
        // Serialize preparation, but never infer readiness from a host PID alone.
        let mut agents = self.persistent_agents.lock().await;
        let running = agents
            .get_mut(device_id)
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !running {
            agents.remove(device_id);
            agents.insert(device_id.into(), launcher.start(device_id).await?);
        }
        let deadline = tokio::time::Instant::now() + AGENT_START_TIMEOUT;
        loop {
            if let Ok(health) = self
                .agent
                .dispatch_for(device_id, "health", json!({}), AGENT_PROBE_TIMEOUT)
                .await
            {
                if compatible(&health, device_id, &version) {
                    return Ok(());
                }
                return Err(RobotError::CommandFailed(
                    "real-iOS helper version/identity mismatch; reinstall this device's helper"
                        .into(),
                ));
            }
            if let Some(child) = agents.get_mut(device_id) {
                if let Some(exit) = child
                    .try_wait()
                    .map_err(|e| RobotError::CommandFailed(e.to_string()))?
                {
                    return Err(RobotError::CommandFailed(format!(
                        "iOS XCTest helper exited with {exit}; inspect derived-data Logs/Agent"
                    )));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(RobotError::CommandFailed(
                    self.agent.connection_diagnostic().await,
                ));
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    async fn dispatch(
        &self,
        device_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, RobotError> {
        self.ensure_device_unlocked(device_id).await?;
        let result = async {
            self.ensure_agent_started(device_id).await?;
            self.agent
                .dispatch_for(device_id, method, params, ACTION_TIMEOUT)
                .await
        }
        .await;
        if result.is_err() {
            // The user may have locked the screen while the operation was running.
            self.ensure_device_unlocked(device_id).await?;
        }
        result
    }

    async fn ensure_device_unlocked(&self, device_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        if crate::connection::device_lock_state(&self.xcrun_path, device_id).await == Some(true) {
            return Err(RobotError::DeviceLocked(device_id.into()));
        }
        Ok(())
    }

    /// Reusable, read-only preflight for QC, REST and MCP callers.
    pub async fn connection_report(
        &self,
        device_id: &str,
        check_xcode: bool,
    ) -> Result<crate::connection::DeviceConnectionReport, RobotError> {
        use crate::connection::{ConnectionCheck, DeviceConnectionReport};
        validate_device_id(device_id)?;
        let (devices, usb) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(4), self.devicectl_devices()),
            crate::connection::usb_attached(device_id)
        );
        let devices = devices.map_err(|_| {
            RobotError::CommandFailed("CoreDevice preflight timed out after 4s".into())
        })??;
        let device = devices
            .iter()
            .find(|d| d.id.eq_ignore_ascii_case(device_id));
        let mut connected = device.is_some_and(|d| d.state == DeviceState::Available);
        let mut probe_error = None;
        if !connected && device.is_some_and(|d| d.state != DeviceState::Unauthorized) {
            match crate::connection::coredevice_connected(&self.xcrun_path, device_id).await {
                Ok(live) => connected = live,
                Err(error) => probe_error = Some(error),
            }
        }
        let mut checks = vec![ConnectionCheck {
            name: "CoreDevice",
            status: if connected { "pass" } else { "fail" },
            detail: if connected {
                "Device connection is live over USB or network".into()
            } else {
                probe_error.unwrap_or_else(|| "Device is offline/unavailable or not trusted; unlock the iPhone and check its trusted USB or Wi-Fi connection".into())
            },
        }];
        let usb_attached = usb.as_ref().ok().copied();
        checks.push(ConnectionCheck {
            name: "USB",
            status: match usb_attached {
                Some(true) => "pass",
                _ if connected => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            detail: match usb {
                Ok(true) => "Matching iPhone detected on USB".into(),
                _ if connected => "USB is not required; live CoreDevice network connection confirmed".into(),
                Ok(false) => "Matching iPhone not found in macOS USB registry; check cable/port or use a live network connection".into(),
                Err(error) => error,
            },
        });
        let destination = if !connected {
            Some(false)
        } else if check_xcode {
            let command = self
                .agent_launcher
                .as_ref()
                .map(|l| l.xcodebuild_path.as_str())
                .unwrap_or("xcodebuild");
            match crate::connection::xcode_destination_ready(
                command,
                &self.runner_project,
                device_id,
            )
            .await
            {
                Ok(ready) => Some(ready),
                Err(error) => {
                    checks.push(ConnectionCheck {
                        name: "Xcode",
                        status: "unknown",
                        detail: error,
                    });
                    None
                }
            }
        } else {
            None
        };
        if destination.is_some() {
            checks.push(ConnectionCheck {
                name: "Xcode destination",
                status: if destination == Some(true) {
                    "pass"
                } else {
                    "fail"
                },
                detail: if destination == Some(true) {
                    "Device is eligible for helper installation"
                } else {
                    "No eligible connected iOS destination; helper build/install was not started"
                }
                .into(),
            });
        }
        let recommended_actions = if !connected {
            vec![
                "check_trusted_usb_or_wifi_connection_and_unlock",
                "trust_this_mac",
            ]
        } else if check_xcode && destination != Some(true) {
            vec!["check_xcode_device_support_and_developer_mode"]
        } else {
            vec![]
        };
        Ok(DeviceConnectionReport {
            device_id: device_id.into(),
            connected,
            usb_attached,
            xcode_destination_ready: destination,
            checks,
            recommended_actions,
        })
    }

    async fn agent_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        let default_project = &self.runner_project;
        let default_derived = default_project.parent().unwrap().join("build");
        let cache = self
            .agent_launcher
            .as_ref()
            .map(|l| l.cache())
            .unwrap_or(RunnerCache {
                project: default_project,
                derived_data: &default_derived,
                simulator: false,
            });
        let expected = cache.version()?;
        let health = self
            .agent
            .dispatch_for(device_id, "health", json!({}), AGENT_PROBE_TIMEOUT)
            .await;
        let mut status = live_status(
            device_id,
            Some(expected.clone()),
            cache.is_current(&expected),
            health,
        );
        if status.state == "disconnected" {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            let log_path = cache
                .derived_data
                .join("Logs/Agent")
                .join(format!("{device_id}-xcodebuild.log"));
            if let Ok(mut file) = tokio::fs::File::open(log_path).await {
                if let Ok(metadata) = file.metadata().await {
                    let _ = file
                        .seek(std::io::SeekFrom::Start(
                            metadata.len().saturating_sub(65536),
                        ))
                        .await;
                    let mut log = Vec::new();
                    if file.take(65536).read_to_end(&mut log).await.is_ok() {
                        apply_trust_warning(&mut status, &String::from_utf8_lossy(&log));
                    }
                }
            }
        }
        if self.agent_launcher.is_none() && status.state == "disconnected" {
            status.state = "unconfigured".into();
            status.detail = "Configure a signed real-iOS helper and a LAN-reachable agent bridge on the Mac before Prepare / Install".into();
        }
        Ok(status)
    }

    async fn devicectl_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        // CoreDevice only guarantees machine-readable output through a JSON file.
        // The table's hostname is allowed to be a friendly name, which is not an
        // Xcode destination UDID.
        let output_path =
            std::env::temp_dir().join(format!("mobile-mcp-devicectl-{}.json", Uuid::new_v4()));
        let output_path_string = output_path.to_string_lossy().into_owned();
        let output = Command::new(&self.xcrun_path)
            .args([
                "devicectl",
                "list",
                "devices",
                "--json-output",
                &output_path_string,
            ])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not run xcrun devicectl: {error}"))
            })?;
        if !output.status.success() {
            return Err(RobotError::CommandFailed(format!(
                "xcrun devicectl list devices failed: {}",
                String::from_utf8_lossy(&output.stderr).trim(),
            )));
        }
        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not read CoreDevice JSON output: {error}"))
            });
        let _ = tokio::fs::remove_file(&output_path).await;
        parse_devicectl_devices(&contents?)
    }

    async fn tap_element_by_filter(&self, device_id: &str, filter: &str) -> Result<(), RobotError> {
        if filter.trim().is_empty() {
            return Err(RobotError::InvalidInput(
                "tap element filter must not be empty".into(),
            ));
        }
        if filter.to_ascii_lowercase().contains("send") {
            let elements = self
                .list_elements_filtered(device_id, Some(filter), Some(20))
                .await?;
            if let Some(element) = select_tappable_element(&elements, filter, false) {
                let width = element.bounds.right - element.bounds.left;
                let height = element.bounds.bottom - element.bounds.top;
                let x = f64::from(element.bounds.left + element.bounds.right) / 2.0;
                let y = if height > width * 3 {
                    f64::from(element.bounds.top) + 12.0_f64.min(f64::from(height) / 2.0)
                } else {
                    f64::from(element.bounds.top + element.bounds.bottom) / 2.0
                };
                return self.tap(device_id, Point { x, y }).await;
            }
        }
        match self
            .dispatch(device_id, "tap-element", json!({ "filter": filter }))
            .await
        {
            Ok(_) => return Ok(()),
            Err(error) if error.to_string().contains("unknown_method") => {}
            Err(error) => return Err(error),
        }

        // Keep compatibility with an older signed runner while the real-device
        // XCTest bundle is being updated. New runners handle the selector in
        // XCTest directly, avoiding a full accessibility dump per tap.
        let elements = self.list_elements(device_id).await?;
        let element = select_tappable_element(&elements, filter, true).ok_or_else(|| {
            RobotError::CommandFailed(format!("could not find UI element matching {filter:?}"))
        })?;
        self.tap(
            device_id,
            Point {
                x: f64::from(element.bounds.left + element.bounds.right) / 2.0,
                y: f64::from(element.bounds.top + element.bounds.bottom) / 2.0,
            },
        )
        .await
    }

    async fn quicktime_device_hint(&self, device_id: &str) -> String {
        if let Ok(hint) = std::env::var("DEVICE_BOT_IOS_QUICKTIME_DEVICE")
            && !hint.trim().is_empty()
        {
            return hint;
        }
        self.devicectl_devices()
            .await
            .ok()
            .and_then(|devices| devices.into_iter().find(|device| device.id == device_id))
            .map(|device| {
                let name = device.name;
                name.split_once(" (")
                    .map_or(name.clone(), |(name, _)| name.to_owned())
            })
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "iPhone".into())
    }
}

#[async_trait]
impl Robot for IosXCTestRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.devicectl_devices().await
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        validate_device_id(device_id)?;
        let output_path =
            std::env::temp_dir().join(format!("mobile-mcp-devicectl-apps-{}.json", Uuid::new_v4()));
        let output_path_string = output_path.to_string_lossy().into_owned();
        let output = Command::new(&self.xcrun_path)
            .args([
                "devicectl",
                "device",
                "info",
                "apps",
                "--device",
                device_id,
                "--include-all-apps",
                "--json-output",
                &output_path_string,
            ])
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not run xcrun devicectl device info apps: {error}"
                ))
            })?;
        if !output.status.success() {
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(RobotError::CommandFailed(format!(
                "xcrun devicectl device info apps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not read devicectl apps JSON output: {error}"
                ))
            });
        let _ = tokio::fs::remove_file(&output_path).await;
        let contents = contents?;
        let apps = serde_json::from_str::<Value>(&contents).map_err(|error| {
            RobotError::CommandFailed(format!("invalid devicectl apps JSON output: {error}"))
        })?;
        Ok(apps)
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        validate_device_id(device_id)?;
        let result = self.dispatch(device_id, "screenshot", json!({})).await?;
        let encoded = result
            .get("pngBase64")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RobotError::CommandFailed("iOS XCTest agent returned no screenshot payload".into())
            })?;
        STANDARD.decode(encoded).map_err(|error| {
            RobotError::CommandFailed(format!(
                "iOS XCTest agent returned invalid screenshot: {error}"
            ))
        })
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        validate_device_id(device_id)?;
        let mut recording = self.recording.lock().await;
        if recording.is_some() {
            return Err(RobotError::InvalidInput(format!(
                "an iOS real-device screen recording is already running for {device_id}"
            )));
        }
        let device_hint = self.quicktime_device_hint(device_id).await;
        let quicktime = QuickTimeRecording::start(&self.artifact_dir, &device_hint).await?;
        let path = quicktime.path();
        *recording = Some(quicktime);
        Ok(path)
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        validate_device_id(device_id)?;
        let Some(recording) = self.recording.lock().await.take() else {
            return Err(RobotError::InvalidInput(format!(
                "no iOS real-device screen recording is running for {device_id}"
            )));
        };
        recording.stop().await
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        validate_app_id(app_id)?;
        self.xcrun(&[
            "devicectl",
            "device",
            "process",
            "launch",
            "--device",
            device_id,
            app_id,
        ])
        .await?;
        self.dispatch(device_id, "launch", json!({ "bundleId": app_id }))
            .await
            .map(|_| ())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        let parsed = Url::parse(url).map_err(|_| RobotError::InvalidInput("invalid URL".into()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        if let Some(app_id) = app_id {
            validate_app_id(app_id)?;
        }
        self.dispatch(device_id, "openUrl", json!({ "url": url, "appId": app_id }))
            .await
            .map(|_| ())
    }

    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        if path.trim().is_empty() {
            return Err(RobotError::InvalidInput(
                "app path must not be empty".into(),
            ));
        }
        self.xcrun(&[
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            device_id,
            path,
        ])
        .await
        .map(|_| ())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        validate_app_id(app_id)?;
        self.xcrun(&[
            "devicectl",
            "device",
            "uninstall",
            "app",
            "--device",
            device_id,
            app_id,
        ])
        .await
        .map(|_| ())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        valid_point(point)?;
        self.dispatch(device_id, "tap", json!({ "x": point.x, "y": point.y }))
            .await
            .map(|_| ())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        if text.is_empty() || text.contains('\0') {
            return Err(RobotError::InvalidInput(
                "text must be non-empty and must not contain NUL".into(),
            ));
        }
        self.dispatch(device_id, "text", json!({ "text": text }))
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
        validate_device_id(device_id)?;
        valid_point(start)?;
        valid_point(end)?;
        self.dispatch(
            device_id,
            "swipe",
            json!({
                "fromX": start.x, "fromY": start.y,
                "toX": end.x, "toY": end.y,
                "durationMs": duration_ms.unwrap_or(100),
            }),
        )
        .await
        .map(|_| ())
    }

    async fn execute_ui_actions(
        &self,
        device_id: &str,
        actions: &[UiAction],
    ) -> Result<(), RobotError> {
        if actions.is_empty() {
            return Err(RobotError::InvalidInput("actions must not be empty".into()));
        }
        validate_device_id(device_id)?;
        for action in actions {
            match action {
                UiAction::Tap { x, y } => self.tap(device_id, Point { x: *x, y: *y }).await?,
                UiAction::TapElement { filter } => {
                    self.tap_element_by_filter(device_id, filter).await?
                }
                UiAction::Type { text } => self.type_text(device_id, text).await?,
                UiAction::Swipe {
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    duration_ms,
                } => {
                    self.swipe(
                        device_id,
                        Point {
                            x: *start_x,
                            y: *start_y,
                        },
                        Point {
                            x: *end_x,
                            y: *end_y,
                        },
                        *duration_ms,
                    )
                    .await?
                }
            }
        }
        Ok(())
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_filtered(device_id, None, None).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        validate_device_id(device_id)?;
        let mut params = serde_json::Map::new();
        if let Some(filter) = filter {
            params.insert("filter".into(), Value::String(filter.into()));
        }
        if let Some(max_results) = max_results {
            params.insert("maxResults".into(), Value::from(max_results));
        }
        let result = self
            .dispatch(device_id, "dump", Value::Object(params))
            .await?;
        let elements = result.get("elements").cloned().ok_or_else(|| {
            RobotError::CommandFailed("iOS XCTest agent returned no element list".into())
        })?;
        let elements: Vec<XCTestElement> = serde_json::from_value(elements).map_err(|error| {
            RobotError::CommandFailed(format!("invalid iOS XCTest element payload: {error}"))
        })?;
        Ok(elements
            .into_iter()
            .map(XCTestElement::into_ui_element)
            .collect())
    }

    async fn list_elements_query(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        let Some(element_type) = query.element_type else {
            return self
                .list_elements_filtered(device_id, query.filter.as_deref(), query.max_results)
                .await;
        };
        validate_device_id(device_id)?;
        let mut params = serde_json::Map::new();
        if let Some(filter) = query.filter.as_ref() {
            params.insert("filter".into(), Value::String(filter.clone()));
        }
        params.insert(
            "elementType".into(),
            Value::String(element_type.as_str().into()),
        );
        if let Some(max_results) = query.max_results {
            params.insert("maxResults".into(), Value::from(max_results));
        }
        let result = self
            .dispatch(device_id, "dump", Value::Object(params))
            .await?;
        if result.get("appliedElementType").and_then(Value::as_str) != Some(element_type.as_str()) {
            return Err(RobotError::Unsupported(
                "iOS XCTest agent did not apply elementType; rebuild and reinstall the runner before querying by type".into(),
            ));
        }
        let elements = result.get("elements").cloned().ok_or_else(|| {
            RobotError::CommandFailed("iOS XCTest agent returned no element list".into())
        })?;
        let elements: Vec<XCTestElement> = serde_json::from_value(elements).map_err(|error| {
            RobotError::CommandFailed(format!("invalid iOS XCTest element payload: {error}"))
        })?;
        let elements = elements
            .into_iter()
            .map(XCTestElement::into_ui_element)
            .collect();
        Ok(apply_element_query(elements, &query))
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        validate_device_id(device_id)?;
        match self
            .dispatch(
                device_id,
                "ime-action",
                json!({ "action": action.as_str() }),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(RobotError::CommandFailed(message)) if message.contains("unknown_method") => {
                Err(RobotError::Unsupported(
                    "iOS XCTest agent does not support IME actions; rebuild and reinstall the runner".into(),
                ))
            }
            Err(error) => Err(error),
        }
    }

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        self.agent_status(device_id).await
    }
    async fn check_device_health(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        let (status, report, screen_locked) = tokio::join!(
            self.agent_status(device_id),
            self.connection_report(device_id, false),
            crate::connection::device_lock_state(&self.xcrun_path, device_id)
        );
        let mut status = status?;
        let report = report?;
        if !report.connected {
            status.ready = false;
            status.state = "offline".into();
            status.detail = report.summary();
        } else if screen_locked == Some(true) {
            status.ready = false;
            status.state = "device_locked".into();
            status.detail = "The iPhone screen is locked. Unlock the iPhone on the device, then retry the operation in QC.".into();
        }
        status.diagnostics = Some(json!(report));
        if let Some(diagnostics) = status.diagnostics.as_mut() {
            diagnostics["screenLocked"] = json!(screen_locked);
            if screen_locked == Some(true) {
                diagnostics["recommendedActions"] = json!(["unlock_device", "retry_operation"]);
            }
        }
        Ok(status)
    }

    async fn prepare_ui_runner(
        &self,
        device_id: &str,
        app_id: Option<&str>,
        reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        let status = self.check_device_health(device_id).await?;
        if status.state == "device_locked" {
            return Ok(status);
        }
        if !reinstall
            && status.ready
            && app_id.is_none_or(|app| status.target_app_id.as_deref() == Some(app))
        {
            return Ok(status);
        }
        let report = self.connection_report(device_id, true).await?;
        if !report.can_install() {
            return Err(RobotError::CommandFailed(report.summary()));
        }
        if self.agent_launcher.is_none() {
            return Err(RobotError::Unsupported("Real-iOS helper setup needs a signed runner and a reachable bridge. Configure DEVICE_BOT_IOS_AGENT_BIND, DEVICE_BOT_IOS_AGENT_HOSTS and the shared agent token on the Mac".into()));
        }
        let target = if app_id.is_some() {
            app_id.map(str::to_owned)
        } else {
            self.agent_status(device_id)
                .await
                .ok()
                .and_then(|s| s.target_app_id)
        };
        if reinstall {
            let _ = self
                .agent
                .dispatch_for(device_id, "stop", json!({}), AGENT_PROBE_TIMEOUT)
                .await;
            if let Some(mut child) = self.persistent_agents.lock().await.remove(device_id) {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        if let Err(error) = self.ensure_agent_started(device_id).await {
            let status = self.check_device_health(device_id).await?;
            if status.state == "device_locked"
                || (status.state == "trust_required"
                    && error.to_string().contains("iOS XCTest helper exited with"))
            {
                return Ok(status);
            }
            return Err(error);
        }
        if let Some(app_id) = target {
            self.launch_app(device_id, &app_id).await?;
        }
        self.agent_status(device_id).await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct XCTestElement {
    node_id: String,
    automation_id: Option<String>,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    role: String,
    bounds: XCTestBounds,
    clickable: bool,
    enabled: bool,
    #[serde(default)]
    focused: Option<bool>,
    #[serde(default)]
    checked: Option<bool>,
    #[serde(default)]
    selected: Option<bool>,
}

#[derive(Deserialize)]
struct XCTestBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl XCTestElement {
    fn into_ui_element(self) -> UiElement {
        let value = self.value.filter(|value| !value.is_empty());
        UiElement {
            class_name: self.role,
            resource_id: self.automation_id.or(Some(self.node_id)),
            text: self.text.or_else(|| value.clone()),
            content_description: self.label.filter(|value| !value.is_empty()),
            bounds: Bounds {
                left: self.bounds.x.round() as i32,
                top: self.bounds.y.round() as i32,
                right: (self.bounds.x + self.bounds.width).round() as i32,
                bottom: (self.bounds.y + self.bounds.height).round() as i32,
            },
            clickable: self.clickable,
            enabled: self.enabled,
            focused: self.focused,
            checked: self.checked,
            selected: self.selected,
            value,
            element_type: None,
        }
    }
}

fn validate_device_id(device_id: &str) -> Result<(), RobotError> {
    if device_id.is_empty()
        || !device_id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-')
    {
        return Err(RobotError::InvalidInput("invalid iOS device id".into()));
    }
    Ok(())
}

fn validate_app_id(app_id: &str) -> Result<(), RobotError> {
    if app_id.is_empty()
        || !app_id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '-' | '_'))
    {
        return Err(RobotError::InvalidInput("invalid iOS app id".into()));
    }
    Ok(())
}

fn valid_point(point: Point) -> Result<(), RobotError> {
    if point.x.is_finite() && point.y.is_finite() && point.x >= 0.0 && point.y >= 0.0 {
        Ok(())
    } else {
        Err(RobotError::InvalidInput(
            "screen coordinates must be finite and non-negative".into(),
        ))
    }
}

fn select_tappable_element<'a>(
    elements: &'a [UiElement],
    filter: &str,
    require_hittable: bool,
) -> Option<&'a UiElement> {
    let filter = filter.to_lowercase();
    elements
        .iter()
        .filter(|element| {
            element.enabled
                && (!require_hittable || element.clickable)
                && element.bounds.right > element.bounds.left
                && element.bounds.bottom > element.bounds.top
                && ui_element_matches(element, &filter)
        })
        .min_by_key(|element| {
            (
                !ui_element_has_exact_match(element, &filter),
                !element.class_name.eq_ignore_ascii_case("button"),
                !element.clickable,
            )
        })
}

fn ui_element_matches(element: &UiElement, filter: &str) -> bool {
    robot_core::element_matches_filter(element, filter)
}

fn ui_element_has_exact_match(element: &UiElement, filter: &str) -> bool {
    [
        Some(element.class_name.as_str()),
        element.resource_id.as_deref(),
        element.text.as_deref(),
        element.content_description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase() == filter)
}

#[derive(Deserialize)]
struct DeviceList {
    result: DeviceListResult,
}

#[derive(Deserialize)]
struct DeviceListResult {
    devices: Vec<CoreDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreDevice {
    identifier: String,
    device_properties: CoreDeviceProperties,
    #[serde(default)]
    connection_properties: Option<CoreConnectionProperties>,
}

#[derive(Deserialize)]
struct CoreDeviceProperties {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreConnectionProperties {
    #[serde(default)]
    local_hostnames: Vec<String>,
    #[serde(default)]
    potential_hostnames: Vec<String>,
    #[serde(default)]
    tunnel_state: Option<String>,
    #[serde(default)]
    pairing_state: Option<String>,
}

fn apply_trust_warning(status: &mut UiRunnerStatus, log: &str) {
    // Logs are appended across launches; an old trust failure must not hide a
    // different error from the latest attempt or override a live heartbeat.
    let latest = log.rsplit("Command line invocation:").next().unwrap_or(log);
    if status.state == "disconnected"
        && [
            "Verify that the Developer App certificate for your account is trusted on your device",
            "profile has not been explicitly trusted by the user",
        ]
        .iter()
        .any(|message| latest.contains(message))
    {
        status.ready = false;
        status.state = "trust_required".into();
        status.detail = "The helper is installed, but iOS blocked its launch. On the iPhone, open Settings > General > VPN & Device Management, select the Developer App certificate and tap Trust. Then return to QC and choose Retry after Trust.".into();
    }
}

fn parse_devicectl_devices(output: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let devices: DeviceList = serde_json::from_str(output).map_err(|error| {
        RobotError::CommandFailed(format!("invalid CoreDevice device JSON: {error}"))
    })?;
    Ok(devices
        .result
        .devices
        .into_iter()
        .filter_map(|device| {
            let connection = device.connection_properties?;
            let state = match (
                connection.pairing_state.as_deref(),
                connection.tunnel_state.as_deref(),
            ) {
                (Some("unpaired"), _) => DeviceState::Unauthorized,
                (_, Some("connected")) => DeviceState::Available,
                (_, Some("unavailable" | "disconnected")) => DeviceState::Offline,
                _ => DeviceState::Unknown,
            };
            let udid = connection
                .local_hostnames
                .into_iter()
                .chain(connection.potential_hostnames)
                .filter_map(|hostname| {
                    hostname
                        .strip_suffix(".coredevice.local")
                        .map(str::to_owned)
                })
                .find(|candidate| {
                    candidate != &device.identifier
                        && candidate
                            .chars()
                            .any(|character| character.is_ascii_digit())
                })?;
            Some(RobotDevice {
                id: udid,
                name: format!("{} ({})", device.device_properties.name, device.identifier),
                platform: Platform::Ios,
                state,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_warning_uses_the_latest_launch_and_never_overrides_a_live_helper() {
        let trust = "Command line invocation:\nxcodebuild test-without-building\nVerify that the Developer App certificate for your account is trusted on your device";
        let mut status = UiRunnerStatus {
            state: "disconnected".into(),
            ..Default::default()
        };
        apply_trust_warning(&mut status, trust);
        assert_eq!(status.state, "trust_required");
        assert!(!status.ready);
        assert!(status.detail.contains("VPN & Device Management"));
        assert!(status.detail.contains("Retry after Trust"));
        let mut ready = UiRunnerStatus {
            ready: true,
            state: "ready".into(),
            ..Default::default()
        };
        apply_trust_warning(&mut ready, trust);
        assert!(ready.ready);
        assert_eq!(ready.state, "ready");
        for log in [
            "Unable to install: device locked".to_owned(),
            format!("{trust}\nCommand line invocation:\nxcodebuild\nerror: device offline"),
        ] {
            let mut status = UiRunnerStatus {
                state: "disconnected".into(),
                ..Default::default()
            };
            apply_trust_warning(&mut status, &log);
            assert_eq!(status.state, "disconnected");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn public_health_exposes_installed_but_untrusted_helper_for_qc() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ios-trust-{}", Uuid::new_v4()));
        let project = root.join("Phone.xcodeproj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.pbxproj"),
            "DEVELOPMENT_TEAM = TEAM123456;",
        )
        .unwrap();
        let xcrun = root.join("xcrun");
        let device = "00008130-000959E43EDA001C";
        let devices = json!({"result":{"devices":[{"identifier":"CORE-ID",
            "deviceProperties":{"name":"Phone"},"connectionProperties":{
                "tunnelState":"connected","pairingState":"paired",
                "potentialHostnames":[format!("{device}.coredevice.local")]}}]}});
        std::fs::write(&xcrun, format!("#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\nprintf '%s' '{}' > \"$last\"\n", devices)).unwrap();
        std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
        let logdir = root.join("build/Logs/Agent");
        std::fs::create_dir_all(&logdir).unwrap();
        std::fs::write(logdir.join(format!("{device}-xcodebuild.log")),
            "Command line invocation:\nxcodebuild\nprofile has not been explicitly trusted by the user").unwrap();
        let robot = IosXCTestRobot::with_artifact_dir(
            xcrun.to_string_lossy(),
            XCTestAgentBridge::new(None),
            root.join("artifacts"),
        )
        .with_runner_project(project);
        let status = robot.check_device_health(device).await.unwrap();
        assert_eq!(status.state, "trust_required");
        assert!(!status.ready);
        assert_eq!(status.diagnostics.unwrap()["connected"], true);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn locked_phone_is_reported_by_health_and_blocks_ui_tools_before_helper_start() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ios-lock-{}", Uuid::new_v4()));
        let project = root.join("Phone.xcodeproj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.pbxproj"),
            "DEVELOPMENT_TEAM = TEAM123456;",
        )
        .unwrap();
        let device = "00008130-000959E43EDA001C";
        let devices = json!({"result":{"devices":[{"identifier":"CORE-ID",
            "deviceProperties":{"name":"Phone"},"connectionProperties":{
                "tunnelState":"connected","pairingState":"paired",
                "potentialHostnames":[format!("{device}.coredevice.local")]}}]}});
        let locked = root.join("locked.json");
        std::fs::write(&locked, r#"{"info":{"outcome":"success"},"result":{"deviceIdentifier":"CORE-ID","passcodeRequired":true,"unlockedSinceBoot":true}}"#).unwrap();
        let xcrun = root.join("xcrun");
        std::fs::write(&xcrun, format!(
            "#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\ncase \"$*\" in\n  'devicectl device info lockState '*) cat '{}' > \"$last\" ;;\n  'devicectl list devices '*) printf '%s' '{}' > \"$last\" ;;\n  *) exit 1 ;;\nesac\n", locked.display(), devices
        )).unwrap();
        std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
        let robot = IosXCTestRobot::with_artifact_dir(
            xcrun.to_string_lossy(),
            XCTestAgentBridge::new(None),
            root.join("artifacts"),
        )
        .with_runner_project(project);
        let status = robot.check_device_health(device).await.unwrap();
        assert_eq!(status.state, "device_locked", "{}", status.detail);
        assert!(!status.ready);
        assert!(status.detail.contains("Unlock"));
        assert_eq!(status.diagnostics.unwrap()["screenLocked"], true);
        let error = robot.take_screenshot(device).await.unwrap_err().to_string();
        assert!(error.starts_with("device_locked:"), "{error}");
        assert!(robot.persistent_agents.lock().await.is_empty());
        std::fs::write(&locked, r#"{"info":{"outcome":"success"},"result":{"deviceIdentifier":"CORE-ID","passcodeRequired":false,"unlockedSinceBoot":true}}"#).unwrap();
        let status = robot.check_device_health(device).await.unwrap();
        assert_ne!(status.state, "device_locked");
        assert_eq!(status.diagnostics.unwrap()["screenLocked"], false);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_coredevice_json_to_xcode_udids() {
        let devices = parse_devicectl_devices(r#"{"result":{"devices":[{"identifier":"F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6","deviceProperties":{"name":"ho-doan"},"connectionProperties":{"potentialHostnames":["00008130-000959E43EDA001C.coredevice.local","F6E1AA55-D8B4-5A55-82D0-0EB3D48F60C6.coredevice.local"]}}]}}"#).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "00008130-000959E43EDA001C");
        assert_eq!(devices[0].platform, Platform::Ios);
    }

    fn element(
        class_name: &str,
        resource_id: Option<&str>,
        content_description: Option<&str>,
        clickable: bool,
        enabled: bool,
    ) -> UiElement {
        UiElement {
            class_name: class_name.into(),
            resource_id: resource_id.map(str::to_owned),
            text: None,
            content_description: content_description.map(str::to_owned),
            bounds: Bounds {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            clickable,
            enabled,
            focused: None,
            checked: None,
            selected: None,
            value: None,
            element_type: None,
        }
    }

    #[test]
    fn tap_element_selection_matches_simulator_semantics() {
        let elements = vec![
            element("textField", None, Some("Private"), true, true),
            element(
                "button",
                Some("PrivateTabGroup"),
                Some("Private, Tab Group"),
                true,
                true,
            ),
            element(
                "button",
                Some("DisabledPrivate"),
                Some("Private"),
                true,
                false,
            ),
        ];

        let selected = select_tappable_element(&elements, "Private, Tab Group", true).unwrap();
        assert_eq!(selected.resource_id.as_deref(), Some("PrivateTabGroup"));
    }

    #[test]
    fn tap_element_selection_rejects_non_hittable_elements() {
        let elements = vec![element(
            "button",
            Some("DoneButton"),
            Some("Done"),
            false,
            true,
        )];

        assert!(select_tappable_element(&elements, "Done", true).is_none());
    }

    #[test]
    fn coordinate_selection_keeps_an_enabled_control_that_is_not_hittable() {
        let elements = vec![element(
            "button",
            Some("action-button"),
            Some("Action"),
            false,
            true,
        )];

        let selected = select_tappable_element(&elements, "Action", false).unwrap();
        assert_eq!(selected.resource_id.as_deref(), Some("action-button"));
        assert!(select_tappable_element(&elements, "Action", true).is_none());
    }

    #[test]
    fn core_launcher_preserves_a_raw_ipv6_advertised_host() {
        let launcher = XCTestAgentLauncher::new(
            "xcodebuild",
            "runner.xcodeproj",
            "derived-data",
            "fd54:ebfd:3d4f::2",
            18787,
            Some("token".into()),
        )
        .unwrap();

        assert_eq!(launcher.hosts, "fd54:ebfd:3d4f::2");
        assert_eq!(launcher.port, 18787);
        assert_eq!(launcher.token.as_deref(), Some("token"));
        assert_eq!(
            launcher.test_environment(),
            vec![
                (
                    "TEST_RUNNER_DEVICE_BOT_HOSTS".into(),
                    "fd54:ebfd:3d4f::2".into()
                ),
                ("TEST_RUNNER_DEVICE_BOT_PORT".into(), "18787".into()),
                ("TEST_RUNNER_DEVICE_BOT_TOKEN".into(), "token".into()),
            ]
        );
    }

    #[test]
    fn core_launcher_rejects_a_loopback_or_link_local_callback_host() {
        for host in ["127.0.0.1", "0.0.0.0", "169.254.1.10", "::1", "fe80::1"] {
            let error = XCTestAgentLauncher::new(
                "xcodebuild",
                "runner.xcodeproj",
                "derived-data",
                host,
                18787,
                Some("token".into()),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("DEVICE_BOT_IOS_AGENT_HOSTS"),
                "{host}"
            );
        }
    }

    #[test]
    fn core_launcher_rejects_a_host_that_is_not_a_host_value() {
        let error = XCTestAgentLauncher::new(
            "xcodebuild",
            "runner.xcodeproj",
            "derived-data",
            "http://192.168.50.56/next",
            18787,
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("DEVICE_BOT_IOS_AGENT_HOSTS"));
    }
    #[test]
    fn unavailable_or_unpaired_coredevices_are_never_available() {
        for (tunnel, pairing, expected) in [
            ("unavailable", "paired", DeviceState::Offline),
            ("disconnected", "paired", DeviceState::Offline),
            ("connected", "unpaired", DeviceState::Unauthorized),
            ("connected", "paired", DeviceState::Available),
            ("connecting", "paired", DeviceState::Unknown),
        ] {
            let devices = parse_devicectl_devices(&json!({"result":{"devices":[{
                "identifier":"CORE-ID", "deviceProperties":{"name":"Phone"},
                "connectionProperties":{"potentialHostnames":["00008130-000959E43EDA001C.coredevice.local"],
                    "tunnelState":tunnel, "pairingState":pairing}
            }]}}).to_string()).unwrap();
            assert_eq!(devices[0].state, expected, "{tunnel}/{pairing}");
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn public_prepare_and_reboot_fail_before_build_when_device_is_unavailable() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ios-preflight-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let xcrun = root.join("xcrun");
        let fixture = json!({"result":{"devices":[{"identifier":"CORE-ID","deviceProperties":{"name":"Phone"},"connectionProperties":{"tunnelState":"unavailable","pairingState":"paired","potentialHostnames":["00008130-000959E43EDA001C.coredevice.local"]}}]}}).to_string();
        std::fs::write(&xcrun, format!("#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\nprintf '%s' '{}' > \"$last\"\n", fixture)).unwrap();
        std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("xctest-runner/MobileMcpRealRunner.xcodeproj");
        let launcher = XCTestAgentLauncher::new(
            "must-not-run-xcodebuild",
            project,
            root.join("build"),
            "192.168.1.2",
            18787,
            Some("test-token".into()),
        )
        .unwrap();
        let robot = IosXCTestRobot::with_artifact_dir(
            xcrun.to_string_lossy(),
            XCTestAgentBridge::new(None),
            root.join("artifacts"),
        )
        .with_agent_launcher(launcher);
        let status = robot
            .check_device_health("00008130-000959E43EDA001C")
            .await
            .unwrap();
        assert!(!status.ready);
        assert_eq!(status.state, "offline");
        assert_eq!(status.diagnostics.unwrap()["connected"], false);
        for reinstall in [false, true] {
            let result = if reinstall {
                robot.reboot_helper("00008130-000959E43EDA001C", None).await
            } else {
                robot
                    .prepare_helper("00008130-000959E43EDA001C", None)
                    .await
            };
            let error = result.unwrap_err().to_string();
            assert!(error.contains("CoreDevice"), "{error}");
            assert!(!error.contains("could not build"), "{error}");
            assert!(robot.persistent_agents.lock().await.is_empty());
            assert!(!root.join("build").exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wifi_preflight_checks_live_device_before_declaring_it_offline() {
        use std::os::unix::fs::PermissionsExt;
        let device = "00008130-000959E43EDA001C";
        let root = std::env::temp_dir().join(format!("ios-wifi-preflight-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let xcrun = root.join("xcrun");
        let cached = json!({"result":{"devices":[{
            "identifier":"CORE-ID", "deviceProperties":{"name":"Phone"},
            "connectionProperties":{"tunnelState":"disconnected","pairingState":"paired",
                "transportType":"localNetwork",
                "potentialHostnames":[format!("{device}.coredevice.local")]}
        }]}});
        let live = json!({"info":{"outcome":"success"},"result":{
            "hardwareProperties":{"udid":device},
            "connectionProperties":{"tunnelState":"connected","pairingState":"paired"}
        }});
        let script = format!(
            "#!/bin/sh\nfor argument in \"$@\"; do last=\"$argument\"; done\ncase \"$*\" in\n  'devicectl list devices '*) printf '%s' '{}' > \"$last\" ;;\n  'devicectl device info details '*) printf '%s' '{}' > \"$last\" ;;\n  *) exit 1 ;;\nesac\n",
            cached, live
        );
        std::fs::write(&xcrun, script).unwrap();
        std::fs::set_permissions(&xcrun, std::fs::Permissions::from_mode(0o700)).unwrap();
        let xcodebuild = root.join("xcodebuild");
        std::fs::write(&xcodebuild, format!(
            "#!/bin/sh\nif [ \"$1\" = '-showdestinations' ]; then\n  printf '%s\\n' 'Available destinations for scheme X:' '{{ platform:iOS, id:{device}, name:Phone }}'\nelse\n  echo 'wifi-helper-build-reached' >&2\n  exit 1\nfi\n"
        )).unwrap();
        std::fs::set_permissions(&xcodebuild, std::fs::Permissions::from_mode(0o700)).unwrap();
        let project = root.join("runner/MobileMcpRealRunner.xcodeproj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("project.pbxproj"),
            "DEVELOPMENT_TEAM = OLDTEAM123;",
        )
        .unwrap();
        std::fs::write(root.join("runner/configure_signing.py"),
            "import pathlib,sys\np=pathlib.Path(sys.argv[2])/'project.pbxproj'\np.write_text(p.read_text().replace('OLDTEAM123','NEWTEAM123'))\nprint('NEWTEAM123')\n"
        ).unwrap();
        std::fs::write(
            root.join("runner/configure_ios_signing.sh"),
            "#!/bin/sh\nexec python3 \"$(dirname \"$0\")/configure_signing.py\" \"$@\"\n",
        )
        .unwrap();
        let launcher = XCTestAgentLauncher::new(
            xcodebuild.to_string_lossy(),
            project,
            root.join("build"),
            "192.168.1.2",
            18787,
            Some("test-token".into()),
        )
        .unwrap();
        let robot = IosXCTestRobot::with_artifact_dir(
            xcrun.to_string_lossy(),
            XCTestAgentBridge::new(None),
            root.join("artifacts"),
        )
        .with_agent_launcher(launcher);
        let report = robot.connection_report(device, true).await.unwrap();
        assert!(
            report.connected,
            "reachable Wi-Fi device was rejected: {}",
            report.summary()
        );
        assert!(report.can_install(), "{}", report.summary());
        assert!(
            !report
                .checks
                .iter()
                .any(|c| c.name == "USB" && c.status != "pass")
        );
        let status = robot.check_device_health(device).await.unwrap();
        assert_ne!(status.state, "offline");
        assert_eq!(status.diagnostics.unwrap()["connected"], true);
        // A fake build failure proves Install passed connection preflight without touching a phone.
        let error = robot
            .prepare_helper(device, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("wifi-helper-build-reached"), "{error}");
        assert_eq!(
            std::fs::read_to_string(
                root.join("runner/MobileMcpRealRunner.xcodeproj/project.pbxproj")
            )
            .unwrap(),
            "DEVELOPMENT_TEAM = NEWTEAM123;"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

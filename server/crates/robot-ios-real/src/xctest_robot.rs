use std::{
    collections::HashMap, fs::File, net::IpAddr, path::PathBuf, process::Stdio, sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use mobile_robot_core::{
    Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiAction, UiElement,
    UiRunnerStatus,
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

use crate::{quicktime_recording::QuickTimeRecording, xctest_agent::XCTestAgentBridge};

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

    async fn start(&self, device_id: &str) -> Result<Child, RobotError> {
        validate_device_id(device_id)?;
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
            .kill_on_drop(true);
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
        }
    }

    pub fn with_agent_launcher(mut self, launcher: XCTestAgentLauncher) -> Self {
        self.agent_launcher = Some(launcher);
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
        let mut agents = self.persistent_agents.lock().await;
        if let Some(agent) = agents.get_mut(device_id) {
            if agent
                .try_wait()
                .map_err(|error| {
                    RobotError::CommandFailed(format!(
                        "could not inspect the real-iOS XCTest agent process: {error}"
                    ))
                })?
                .is_none()
            {
                return Ok(());
            }
            agents.remove(device_id);
        }
        let agent = launcher.start(device_id).await?;
        agents.insert(device_id.to_owned(), agent);
        Ok(())
    }

    async fn dispatch(
        &self,
        device_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, RobotError> {
        self.ensure_agent_started(device_id).await?;
        self.agent.dispatch(method, params, ACTION_TIMEOUT).await
    }

    async fn agent_exit_detail(&self, device_id: &str) -> Result<Option<String>, RobotError> {
        let mut agents = self.persistent_agents.lock().await;
        let Some(agent) = agents.get_mut(device_id) else {
            return Ok(None);
        };
        let Some(status) = agent.try_wait().map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not inspect the real-iOS XCTest agent process: {error}"
            ))
        })?
        else {
            return Ok(None);
        };
        agents.remove(device_id);
        let log_path = self
            .agent_launcher
            .as_ref()
            .map(|launcher| {
                launcher
                    .derived_data
                    .join("Logs/Agent")
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "the XCTest derived-data Logs/Agent directory".into());
        Ok(Some(format!(
            "the auto-started iOS XCTest agent exited with {status}; inspect {log_path}"
        )))
    }

    async fn agent_status(&self, device_id: &str) -> UiRunnerStatus {
        if let Err(error) = self.ensure_agent_started(device_id).await {
            return UiRunnerStatus {
                ready: false,
                detail: error.to_string(),
            };
        }
        let deadline = tokio::time::Instant::now()
            + if self.agent_launcher.is_some() {
                AGENT_START_TIMEOUT
            } else {
                AGENT_PROBE_TIMEOUT
            };
        loop {
            if self
                .agent
                .dispatch("health", json!({}), AGENT_PROBE_TIMEOUT)
                .await
                .is_ok()
            {
                return UiRunnerStatus {
                    ready: true,
                    detail: "XCTest agent connected".into(),
                };
            }
            match self.agent_exit_detail(device_id).await {
                Ok(Some(detail)) => {
                    return UiRunnerStatus {
                        ready: false,
                        detail,
                    };
                }
                Ok(None) => {}
                Err(error) => {
                    return UiRunnerStatus {
                        ready: false,
                        detail: error.to_string(),
                    };
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return UiRunnerStatus {
                    ready: false,
                    detail: self.agent.connection_diagnostic().await,
                };
            }
            sleep(Duration::from_secs(1)).await;
        }
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

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        validate_device_id(device_id)?;
        Ok(self.agent_status(device_id).await)
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
        UiElement {
            class_name: self.role,
            resource_id: self.automation_id.or(Some(self.node_id)),
            text: self.text.or(self.value),
            content_description: self.label,
            bounds: Bounds {
                left: self.bounds.x.round() as i32,
                top: self.bounds.y.round() as i32,
                right: (self.bounds.x + self.bounds.width).round() as i32,
                bottom: (self.bounds.y + self.bounds.height).round() as i32,
            },
            clickable: self.clickable,
            enabled: self.enabled,
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
    [
        Some(element.class_name.as_str()),
        element.resource_id.as_deref(),
        element.text.as_deref(),
        element.content_description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(filter))
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
                state: DeviceState::Available,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

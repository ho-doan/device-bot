use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use robot_core::{
    DeviceState, ElementQuery, Point, Robot, RobotDevice, RobotError, UiElement, UiRunnerStatus,
    WebContext, apply_element_query,
};
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
    signing_issue::SigningIssue,
    xctest_agent::XCTestAgentBridge,
};

mod adapter;
mod launcher;
mod protocol;

use launcher::RunnerStartError;
pub use launcher::XCTestAgentLauncher;
use protocol::{XCTestElement, parse_devicectl_devices};
const ACTION_TIMEOUT: Duration = Duration::from_secs(45);
const AGENT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const AGENT_START_TIMEOUT: Duration = Duration::from_secs(60);
const BRIDGE_UNREACHABLE: &str = "bridge_unreachable";

#[derive(Clone)]
enum PreparedLiveSource {
    Usb(crate::live::Source),
    Wifi(Arc<crate::wifi_video::WifiSource>),
}

pub struct IosXCTestRobot {
    xcrun_path: String,
    agent: XCTestAgentBridge,
    artifact_dir: PathBuf,
    recording: Arc<Mutex<Option<QuickTimeRecording>>>,
    agent_launcher: Option<XCTestAgentLauncher>,
    persistent_agents: Arc<Mutex<HashMap<String, Child>>>,
    signing_issues: Arc<Mutex<HashMap<String, SigningIssue>>>,
    runner_project: PathBuf,
    live_sources: Arc<Mutex<HashMap<String, PreparedLiveSource>>>,
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
            signing_issues: Arc::default(),
            live_sources: Arc::default(),
            runner_project: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("xctest-runner/MobileMcpRealRunner.xcodeproj"),
        }
    }

    pub fn with_agent_launcher(mut self, launcher: XCTestAgentLauncher) -> Self {
        self.runner_project = launcher.project.clone();
        self.agent_launcher = Some(launcher);
        self
    }

    async fn prepare_wifi_source(
        &self,
        device_id: &str,
        logical: (f64, f64),
    ) -> Result<Arc<crate::wifi_video::WifiSource>, RobotError> {
        let launcher = self.agent_launcher.as_ref().ok_or_else(|| RobotError::Unsupported(
            "iOS Wi-Fi Live requires a LAN-reachable agent bridge; configure DEVICE_BOT_IOS_AGENT_BIND, DEVICE_BOT_IOS_AGENT_HOSTS and DEVICE_BOT_IOS_AGENT_TOKEN".into()
        ))?;
        let (source, created) = self.agent.wifi_video.prepare(device_id, logical).await;
        if created {
            let host = launcher
                .hosts
                .split(',')
                .next()
                .expect("validated callback host");
            if let Err(error) = crate::wifi_app::launch(
                &launcher.project,
                &launcher.xcodebuild_path,
                &self.xcrun_path,
                &self.artifact_dir,
                device_id,
                || source.configuration(host, launcher.port),
            )
            .await
            {
                source.stop();
                return Err(error);
            }
            eprintln!(
                "iOS Wi-Fi Live: Device Bot Broadcast is open on the iPhone. Allow Local Network, tap the record button, then Start Broadcast."
            );
        }
        source.ready(Duration::from_secs(60)).await?;
        Ok(source)
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
                self.signing_issues.lock().await.remove(device_id);
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
        let signing_team =
            match crate::configure_xctest_signing(&launcher.project, None, None).await {
                Ok(team) => team,
                Err(error)
                    if error
                        .to_string()
                        .contains("No valid iOS development signing identity") =>
                {
                    let preferred = std::env::var("DEVICE_BOT_IOS_PERSONAL_TEAM")
                        .ok()
                        .or_else(|| std::env::var("DEVICE_BOT_IOS_TEAM").ok());
                    return Err(self
                        .remember_signing_issue(
                            device_id,
                            SigningIssue::identity_required(preferred.as_deref()),
                        )
                        .await);
                }
                Err(error) => return Err(error),
            };
        let version = launcher.cache().version()?;
        // Serialize preparation, but never infer readiness from a host PID alone.
        let mut agents = self.persistent_agents.lock().await;
        let running = agents
            .get_mut(device_id)
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !running {
            agents.remove(device_id);
            self.signing_issues.lock().await.remove(device_id);
            let child = match launcher.start(device_id, signing_team.as_deref()).await {
                Ok(child) => child,
                Err(RunnerStartError::Signing(issue)) => {
                    return Err(self.remember_signing_issue(device_id, issue).await);
                }
                Err(RunnerStartError::Other(error)) => return Err(error),
            };
            agents.insert(device_id.into(), child);
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
                let mut status = UiRunnerStatus {
                    state: "disconnected".into(),
                    ..Default::default()
                };
                read_runner_warning(&mut status, &launcher.derived_data, device_id).await;
                if status.state == BRIDGE_UNREACHABLE {
                    return Err(RobotError::CommandFailed(status.detail));
                }
                return Err(RobotError::CommandFailed(
                    self.agent.connection_diagnostic().await,
                ));
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    async fn remember_signing_issue(&self, device_id: &str, issue: SigningIssue) -> RobotError {
        let error = RobotError::CommandFailed(issue.detail().to_owned());
        self.signing_issues
            .lock()
            .await
            .insert(device_id.into(), issue);
        error
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

    async fn agent_status(
        &self,
        device_id: &str,
        timeout: Duration,
    ) -> Result<UiRunnerStatus, RobotError> {
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
            .dispatch_for(device_id, "health", json!({}), timeout)
            .await;
        let mut status = live_status(
            device_id,
            Some(expected.clone()),
            cache.is_current(&expected),
            health,
        );
        if status.state == "disconnected" {
            if let Some(issue) = self.signing_issues.lock().await.get(device_id).cloned() {
                issue.apply(&mut status);
                return Ok(status);
            }
            read_runner_warning(&mut status, cache.derived_data, device_id).await;
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

    async fn webview_elements(
        &self,
        device_id: &str,
        context: &WebContext,
        query: &ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        context.validate()?;
        let bundle_id = context.app_id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| {
            RobotError::InvalidInput(
                "web context needs appId for the iOS WebView; native accessibility is not used as a fallback"
                    .into(),
            )
        })?;
        let elements = crate::webview::read_dom(device_id, bundle_id, "usbmux", context).await?;
        let scoped = ElementQuery {
            include_frames: true,
            ..query.clone()
        };
        Ok(apply_element_query(elements, &scoped))
    }

    async fn webview_edit(
        &self,
        device_id: &str,
        context: &WebContext,
        action: &str,
        text: &str,
    ) -> Result<(), RobotError> {
        context.validate()?;
        let bundle_id = context
            .app_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                RobotError::InvalidInput("web context needs appId for the iOS WebView".into())
            })?;
        crate::webview::edit_dom(device_id, bundle_id, "usbmux", context, action, text).await
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

async fn read_runner_warning(status: &mut UiRunnerStatus, derived_data: &Path, device_id: &str) {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    const LOG_TAIL: u64 = 65_536;
    let path = derived_data
        .join("Logs/Agent")
        .join(format!("{device_id}-xcodebuild.log"));
    if let Ok(mut file) = tokio::fs::File::open(path).await
        && let Ok(metadata) = file.metadata().await
        && file
            .seek(std::io::SeekFrom::Start(
                metadata.len().saturating_sub(LOG_TAIL),
            ))
            .await
            .is_ok()
    {
        let mut log = Vec::new();
        if file.take(LOG_TAIL).read_to_end(&mut log).await.is_ok() {
            apply_runner_warning(status, &String::from_utf8_lossy(&log));
        }
    }
}

fn apply_runner_warning(status: &mut UiRunnerStatus, log: &str) {
    if status.state != "disconnected" {
        return;
    }
    // Logs are appended across launches. A previous attempt must not hide
    // a different error in the latest one or override a live heartbeat.
    let latest = log.rsplit("Command line invocation:").next().unwrap_or(log);
    if [
        "Verify that the Developer App certificate for your account is trusted on your device",
        "profile has not been explicitly trusted by the user",
    ]
    .iter()
    .any(|message| latest.contains(message))
    {
        status.ready = false;
        status.state = "trust_required".into();
        status.detail = "The helper is installed, but iOS blocked its launch. On the iPhone, open Settings > General > VPN & Device Management, select the Developer App certificate and tap Trust. Then return to QC and choose Retry after Trust.".into();
    } else if let Some(address) = unreachable_bridge(latest) {
        status.ready = false;
        status.state = BRIDGE_UNREACHABLE.into();
        status.detail = format!(
            "XCTest could not reach the Mac bridge at {address}. Connect the iPhone and Mac to the same local network. If they already are, check Device Bot’s Local Network permission on the iPhone and that the Mac’s bridge address is current."
        );
    }
}

fn unreachable_bridge(log: &str) -> Option<String> {
    for line in log.lines().rev() {
        if line.contains("Device Bot bridge reachable") {
            return None;
        }
        let endpoint = if let Some((_, rest)) = line.split_once("Device Bot bridge unreachable (") {
            rest.split_once(')').map(|(url, _)| url)
        } else if let Some((_, rest)) = line.split_once("Device Bot bridge request failed (GET ") {
            // Compatibility with already-installed runners that only printed a
            // localized description. Do not classify HTTP/authentication errors.
            rest.split_once("): ")
                .filter(|(_, error)| *error == "The request timed out.")
                .map(|(url, _)| url)
        } else {
            None
        };
        if let Some(url) = endpoint.and_then(|url| Url::parse(url).ok())
            && url.scheme() == "http"
            && url.path() == "/next"
            && let (Some(host), Some(port)) = (url.host(), url.port_or_known_default())
        {
            return Some(format!("{host}:{port}"));
        }
    }
    None
}

#[cfg(test)]
mod tests;

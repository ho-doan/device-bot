//! iOS Simulator driver backed by Xcode's `xcrun simctl` and a bundled XCTest runner.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use robot_core::{
    Bounds, DeviceState, ElementQuery, ImeAction, Platform, Point, Robot, RobotDevice, RobotError,
    UiAction, UiElement, UiRunnerStatus, apply_element_query,
};
use robot_ios_real::{
    XCTestAgentBridge,
    runner_cache::{RunnerCache, compatible, live_status},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::Mutex,
    time::{Instant, sleep},
};

pub mod health;
static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const XCTEST_BUILD_TIMEOUT: Duration = Duration::from_secs(90);
const XCTEST_RUNNER_BUNDLE_ID: &str = "com.mobilemcp.simulatorrunner.tests.xctrunner";

#[derive(Clone)]
pub struct IosSimulatorRobot {
    xcrun_path: String,
    xcodebuild_path: String,
    xctest_project: PathBuf,
    artifact_dir: PathBuf,
    recordings: Arc<Mutex<HashMap<String, ActiveRecording>>>,
    active_apps: Arc<Mutex<HashMap<String, String>>>,
    xctest_lock: Arc<Mutex<()>>,
    simulator_agent: Option<XCTestAgentBridge>,
    simulator_agent_host: String,
    simulator_agent_port: u16,
    persistent_agents: Arc<Mutex<HashMap<String, Child>>>,
    agent_apps: Arc<Mutex<HashMap<String, String>>>,
}

struct ActiveRecording {
    child: Child,
    path: PathBuf,
}

impl IosSimulatorRobot {
    pub fn new(xcrun_path: impl Into<String>, artifact_dir: impl Into<PathBuf>) -> Self {
        Self::with_xctest_runner(
            xcrun_path,
            "xcodebuild",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("xctest-runner/MobileMcpSimulatorRunner.xcodeproj"),
            artifact_dir,
        )
    }

    /// Creates a simulator driver with explicit XCTest runner locations.
    pub fn with_xctest_runner(
        xcrun_path: impl Into<String>,
        xcodebuild_path: impl Into<String>,
        xctest_project: impl Into<PathBuf>,
        artifact_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::with_xctest_runner_and_agent_option(
            xcrun_path,
            xcodebuild_path,
            xctest_project,
            artifact_dir,
            None,
            "127.0.0.1",
            0,
        )
    }

    pub fn with_xctest_runner_and_agent(
        xcrun_path: impl Into<String>,
        xcodebuild_path: impl Into<String>,
        xctest_project: impl Into<PathBuf>,
        artifact_dir: impl Into<PathBuf>,
        simulator_agent: XCTestAgentBridge,
        simulator_agent_host: impl Into<String>,
        simulator_agent_port: u16,
    ) -> Self {
        Self::with_xctest_runner_and_agent_option(
            xcrun_path,
            xcodebuild_path,
            xctest_project,
            artifact_dir,
            Some(simulator_agent),
            simulator_agent_host,
            simulator_agent_port,
        )
    }

    fn with_xctest_runner_and_agent_option(
        xcrun_path: impl Into<String>,
        xcodebuild_path: impl Into<String>,
        xctest_project: impl Into<PathBuf>,
        artifact_dir: impl Into<PathBuf>,
        simulator_agent: Option<XCTestAgentBridge>,
        simulator_agent_host: impl Into<String>,
        simulator_agent_port: u16,
    ) -> Self {
        Self {
            xcrun_path: xcrun_path.into(),
            xcodebuild_path: xcodebuild_path.into(),
            xctest_project: xctest_project.into(),
            artifact_dir: artifact_dir.into(),
            recordings: Arc::default(),
            active_apps: Arc::default(),
            xctest_lock: Arc::default(),
            simulator_agent,
            simulator_agent_host: simulator_agent_host.into(),
            simulator_agent_port,
            persistent_agents: Arc::default(),
            agent_apps: Arc::default(),
        }
    }

    async fn simctl(&self, args: &[String]) -> Result<Vec<u8>, RobotError> {
        let output = Command::new(&self.xcrun_path)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start xcrun: {error}"))
            })?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(RobotError::CommandFailed(format!(
            "simctl exited with status {}: {}",
            output
                .status
                .code()
                .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    async fn simctl_json(&self, args: &[String]) -> Result<Value, RobotError> {
        let plist = self.simctl(args).await?;
        let mut converter = Command::new("plutil")
            .args(["-convert", "json", "-o", "-", "--", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start plutil: {error}"))
            })?;
        let mut stdin = converter
            .stdin
            .take()
            .ok_or_else(|| RobotError::CommandFailed("could not open plutil stdin".into()))?;
        stdin.write_all(&plist).await.map_err(|error| {
            RobotError::CommandFailed(format!("could not write simctl plist to plutil: {error}"))
        })?;
        drop(stdin);
        let output = converter.wait_with_output().await.map_err(|error| {
            RobotError::CommandFailed(format!("could not wait for plutil: {error}"))
        })?;
        if !output.status.success() {
            return Err(RobotError::CommandFailed(format!(
                "plutil exited with status {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!("plutil returned invalid simctl app JSON: {error}"))
        })
    }

    fn valid_id(value: &str, label: &str) -> Result<(), RobotError> {
        if value.trim().is_empty() {
            Err(RobotError::InvalidInput(format!(
                "{label} must not be empty"
            )))
        } else {
            Ok(())
        }
    }

    fn valid_device_id(value: &str) -> Result<(), RobotError> {
        Self::valid_id(value, "device id")?;
        if value.starts_with('-')
            || !value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(RobotError::InvalidInput(
                "device id must be a simulator UDID".into(),
            ));
        }
        Ok(())
    }

    fn valid_point(value: Point, label: &str) -> Result<(), RobotError> {
        if !value.x.is_finite() || !value.y.is_finite() || value.x < 0.0 || value.y < 0.0 {
            return Err(RobotError::InvalidInput(format!(
                "{label} coordinates must be finite, non-negative screen points"
            )));
        }
        Ok(())
    }

    fn next_recording_path(&self, device_id: &str) -> PathBuf {
        self.artifact_dir.join(format!(
            "mobile-mcp-ios-recording-{device_id}-{}-{}.mp4",
            std::process::id(),
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn xctest_derived_data() -> PathBuf {
        std::env::var_os("DEVICE_BOT_IOS_XCTEST_DERIVED_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join("mobile-mcp-ios-xctest-derived-data-persistent-agent")
            })
    }

    #[cfg(test)]
    fn xctest_runner_ready(derived_data: &Path) -> bool {
        let products = derived_data.join("Build/Products/Debug-iphonesimulator");
        products.join("MobileMcpSimulatorRunnerHost.app").is_dir()
            && products
                .join("MobileMcpSimulatorRunnerUITests-Runner.app")
                .is_dir()
    }

    async fn ensure_xctest_runner(
        &self,
        project: &str,
        destination: &str,
        derived_data: &Path,
    ) -> Result<(), RobotError> {
        let cache = RunnerCache {
            project: &self.xctest_project,
            derived_data,
            simulator: true,
        };
        let version = cache.version()?;
        if cache.is_current(&version) {
            return Ok(());
        }
        let derived_data = derived_data.to_string_lossy().into_owned();
        let output = tokio::time::timeout(
            XCTEST_BUILD_TIMEOUT,
            Command::new(&self.xcodebuild_path)
                .args([
                    "build-for-testing",
                    "-project",
                    project,
                    "-scheme",
                    "MobileMcpSimulatorRunner",
                    "-destination",
                    destination,
                    "-derivedDataPath",
                    &derived_data,
                ])
                .arg(format!("DEVICE_BOT_BUILD_ID={version}"))
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| {
            RobotError::CommandFailed(format!(
                "bundled XCTest runner build timed out after {} seconds",
                XCTEST_BUILD_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not build bundled XCTest runner at {}: {error}",
                self.xcodebuild_path
            ))
        })?;

        if output.status.success() {
            return cache.mark_current(&version);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let details = if stderr.is_empty() { stdout } else { stderr };
        Err(RobotError::CommandFailed(format!(
            "bundled XCTest runner build failed with status {}: {details}",
            output
                .status
                .code()
                .map_or_else(|| "unknown".into(), |code| code.to_string())
        )))
    }

    /// Resolves an artifact path before passing it into the XCTest process.
    ///
    /// XCTest runs in a different working directory from the MCP server, so a
    /// relative result path would be resolved inside the test runner instead of
    /// the server's artifact directory.
    fn absolute_path(path: &Path) -> Result<PathBuf, RobotError> {
        if path.is_absolute() {
            return Ok(path.to_owned());
        }
        std::env::current_dir()
            .map(|working_directory| working_directory.join(path))
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not resolve XCTest artifact directory: {error}"
                ))
            })
    }

    async fn active_app(&self, device_id: &str) -> Result<String, RobotError> {
        Self::valid_device_id(device_id)?;
        if let Some(app) = self.active_apps.lock().await.get(device_id).cloned() {
            return Ok(app);
        }
        // A compatible helper outlives the MCP process and remembers its app.
        if let Ok(status) = self.ui_runner_status(device_id).await {
            if status.ready {
                return Ok(status.target_app_id.unwrap());
            }
        }
        Err(RobotError::InvalidInput(format!(
            "no active app is known for simulator {device_id}; launch an app before using UI tools"
        )))
    }

    async fn stop_persistent_agent(&self, device_id: &str) {
        let stopped = if let Some(agent) = &self.simulator_agent {
            agent
                .dispatch_for(device_id, "stop", json!({}), Duration::from_secs(2))
                .await
                .is_ok()
        } else {
            false
        };
        if let Some(mut child) = self.persistent_agents.lock().await.remove(device_id) {
            if tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .is_err()
            {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
        if !stopped {
            let _ = self
                .simctl(&[
                    "simctl".into(),
                    "terminate".into(),
                    device_id.into(),
                    XCTEST_RUNNER_BUNDLE_ID.into(),
                ])
                .await;
        }
    }

    async fn wait_for_persistent_agent(
        &self,
        agent: &XCTestAgentBridge,
        device_id: &str,
        version: &str,
        timeout: Duration,
    ) -> Result<Value, RobotError> {
        let deadline = Instant::now() + timeout;
        loop {
            match agent
                .dispatch_for(device_id, "health", json!({}), Duration::from_secs(2))
                .await
            {
                Ok(value) if compatible(&value, device_id, version) => return Ok(value),
                Ok(_) => {
                    return Err(RobotError::CommandFailed(
                        "XCTest helper version/identity mismatch after preparation".into(),
                    ));
                }
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    sleep(Duration::from_millis(250)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn ensure_agent_app(
        &self,
        device_id: &str,
        target_app_id: &str,
        agent: &XCTestAgentBridge,
        current_app: &str,
    ) -> Result<(), RobotError> {
        if target_app_id.is_empty() {
            return Ok(());
        }
        if current_app != target_app_id {
            // Reject a missing target before XCUIApplication.launch can fail
            // the long-lived test and tear down an otherwise healthy helper.
            self.simctl(&[
                "simctl".into(),
                "get_app_container".into(),
                device_id.into(),
                target_app_id.into(),
                "app".into(),
            ])
            .await?;
            agent
                .dispatch_for(
                    device_id,
                    "launch",
                    json!({ "bundleId": target_app_id }),
                    Duration::from_secs(20),
                )
                .await?;
            self.agent_apps
                .lock()
                .await
                .insert(device_id.into(), target_app_id.into());
        }
        Ok(())
    }

    async fn ensure_persistent_agent(
        &self,
        device_id: &str,
        target_app_id: &str,
        project: &str,
        destination: &str,
        derived_data: &Path,
    ) -> Result<(), RobotError> {
        let agent = self.simulator_agent.clone().ok_or_else(|| {
            RobotError::CommandFailed(
                "iOS Simulator XCTest agent is not configured for this server".into(),
            )
        })?;

        let version = RunnerCache {
            project: &self.xctest_project,
            derived_data,
            simulator: true,
        }
        .version()?;
        if let Ok(health) = agent
            .dispatch_for(device_id, "health", json!({}), Duration::from_secs(2))
            .await
        {
            if compatible(&health, device_id, &version) {
                self.ensure_agent_app(
                    device_id,
                    target_app_id,
                    &agent,
                    if health["appForeground"] == true {
                        health["bundleId"].as_str().unwrap_or_default()
                    } else {
                        ""
                    },
                )
                .await?;
                return Ok(());
            }
            self.stop_persistent_agent(device_id).await;
        }

        let agent_is_running = {
            let mut agents = self.persistent_agents.lock().await;
            match agents.get_mut(device_id) {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) | Err(_) => {
                        agents.remove(device_id);
                        false
                    }
                    Ok(None) => true,
                },
                None => false,
            }
        };
        if agent_is_running {
            if let Ok(health) = self
                .wait_for_persistent_agent(&agent, device_id, &version, Duration::from_secs(15))
                .await
            {
                let current_app = if health["appForeground"] == true {
                    health["bundleId"].as_str().unwrap_or_default()
                } else {
                    ""
                };
                self.ensure_agent_app(device_id, target_app_id, &agent, current_app)
                    .await?;
                return Ok(());
            }
            self.stop_persistent_agent(device_id).await;
        } else {
            // A previous MCP process may have left the agent app running while
            // its Child handle was dropped. Terminate only that helper before
            // starting a replacement; the compiled products stay cached.
            let _ = self
                .simctl(&[
                    "simctl".into(),
                    "terminate".into(),
                    device_id.into(),
                    XCTEST_RUNNER_BUNDLE_ID.into(),
                ])
                .await;
        }

        self.ensure_xctest_runner(project, destination, derived_data)
            .await?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest artifact directory: {error}"
                ))
            })?;
        let artifact_dir = Self::absolute_path(&self.artifact_dir)?;
        let stdout_path = artifact_dir.join(format!(
            "mobile-mcp-ios-xctest-agent-{device_id}.stdout.log"
        ));
        let stderr_path = artifact_dir.join(format!(
            "mobile-mcp-ios-xctest-agent-{device_id}.stderr.log"
        ));
        let stdout = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&stdout_path)
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest agent stdout log {}: {error}",
                    stdout_path.display()
                ))
            })?;
        let stderr = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest agent stderr log {}: {error}",
                    stderr_path.display()
                ))
            })?;
        let derived_data_argument = derived_data.to_string_lossy().into_owned();
        let mut command = Command::new(&self.xcodebuild_path);
        command
            .args([
                "test-without-building",
                "-project",
                project,
                "-scheme",
                "MobileMcpSimulatorRunner",
                "-destination",
                destination,
                "-only-testing:MobileMcpSimulatorRunnerUITests/MobileMcpSimulatorRunnerUITests/testServe",
                "-derivedDataPath",
                &derived_data_argument,
            ])
            .env("TEST_RUNNER_DEVICE_BOT_DEVICE_ID", device_id)
            .env("TEST_RUNNER_DEVICE_BOT_HOSTS", &self.simulator_agent_host)
            .env(
                "TEST_RUNNER_DEVICE_BOT_PORT",
                self.simulator_agent_port.to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        if let Ok(token) = std::env::var("DEVICE_BOT_IOS_SIM_AGENT_TOKEN") {
            command.env("TEST_RUNNER_DEVICE_BOT_TOKEN", token);
        }
        // Keep the long-lived XCTest process outside the MCP server's process
        // group so stopping one MCP session does not stop the cached agent.
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn().map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not start persistent XCTest agent at {}: {error}",
                self.xcodebuild_path
            ))
        })?;
        self.persistent_agents
            .lock()
            .await
            .insert(device_id.into(), child);

        let health = self
            .wait_for_persistent_agent(&agent, device_id, &version, Duration::from_secs(60))
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "persistent XCTest agent did not become ready: {error}"
                ))
            })?;
        let current_app = if health["appForeground"] == true {
            health["bundleId"].as_str().unwrap_or_default()
        } else {
            ""
        };
        self.ensure_agent_app(device_id, target_app_id, &agent, current_app)
            .await?;
        Ok(())
    }

    async fn run_xctest(
        &self,
        device_id: &str,
        target_app_id: &str,
        action: &str,
        values: &[(&str, String)],
        timeout: Option<Duration>,
    ) -> Result<Value, RobotError> {
        Self::valid_device_id(device_id)?;
        if !self.xctest_project.is_dir() {
            return Err(RobotError::CommandFailed(format!(
                "bundled XCTest project was not found at {}",
                self.xctest_project.display()
            )));
        }
        let derived_data = Self::xctest_derived_data();
        let project = self.xctest_project.to_string_lossy().into_owned();
        let destination = format!("platform=iOS Simulator,id={device_id}");
        let _xctest_lock = self.xctest_lock.lock().await;
        self.ensure_persistent_agent(
            device_id,
            target_app_id,
            &project,
            &destination,
            &derived_data,
        )
        .await?;
        let agent = self.simulator_agent.clone().ok_or_else(|| {
            RobotError::CommandFailed(
                "iOS Simulator XCTest agent is not configured for this server".into(),
            )
        })?;
        let params = values
            .iter()
            .filter_map(|(name, value)| match *name {
                "DEVICE_BOT_X" => Some(("x", json!(value.parse::<f64>().ok()?))),
                "DEVICE_BOT_Y" => Some(("y", json!(value.parse::<f64>().ok()?))),
                "DEVICE_BOT_END_X" => Some(("toX", json!(value.parse::<f64>().ok()?))),
                "DEVICE_BOT_END_Y" => Some(("toY", json!(value.parse::<f64>().ok()?))),
                "DEVICE_BOT_DURATION_SECONDS" => Some((
                    "durationMs",
                    json!(value.parse::<f64>().ok()?.mul_add(1000.0, 0.0) as u64),
                )),
                "DEVICE_BOT_TEXT" => Some(("text", json!(value))),
                "DEVICE_BOT_ELEMENT_FILTER" => Some(("filter", json!(value))),
                "DEVICE_BOT_ELEMENT_TYPE" => Some(("elementType", json!(value))),
                "DEVICE_BOT_IME_ACTION" => Some(("action", json!(value))),
                "DEVICE_BOT_MAX_RESULTS" => {
                    Some(("maxResults", json!(value.parse::<usize>().ok()?)))
                }
                _ => None,
            })
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<serde_json::Map<_, _>>();
        let timeout = timeout.unwrap_or(Duration::from_secs(45));
        match action {
            "tap" => {
                agent
                    .dispatch_for(device_id, "tap", Value::Object(params), timeout)
                    .await
            }
            "tap-element" => {
                agent
                    .dispatch_for(device_id, "tap-element", Value::Object(params), timeout)
                    .await
            }
            "type" => {
                agent
                    .dispatch_for(device_id, "text", Value::Object(params), timeout)
                    .await
            }
            "ime-action" => {
                agent
                    .dispatch_for(device_id, "ime-action", Value::Object(params), timeout)
                    .await
            }
            "swipe" => {
                let mut params = params;
                if let Some(value) = params.remove("x") {
                    params.insert("fromX".into(), value);
                }
                if let Some(value) = params.remove("y") {
                    params.insert("fromY".into(), value);
                }
                agent
                    .dispatch_for(device_id, "swipe", Value::Object(params), timeout)
                    .await
            }
            "list-elements" => {
                agent
                    .dispatch_for(device_id, "dump", Value::Object(params), timeout)
                    .await
            }
            _ => Err(RobotError::Unsupported(format!(
                "persistent XCTest agent action {action} is not supported"
            ))),
        }
    }

    async fn tap_element_by_filter(&self, device_id: &str, filter: &str) -> Result<(), RobotError> {
        let elements = self
            .list_elements_filtered(device_id, Some(filter), Some(20))
            .await?;
        let element = elements
            .into_iter()
            .find(|element| {
                element.enabled
                    && element.clickable
                    && element.bounds.right > element.bounds.left
                    && element.bounds.bottom > element.bounds.top
            })
            .ok_or_else(|| {
                RobotError::CommandFailed(format!("could not find UI element matching {filter:?}"))
            })?;
        let width = element.bounds.right - element.bounds.left;
        let height = element.bounds.bottom - element.bounds.top;
        let x = f64::from(element.bounds.left + element.bounds.right) / 2.0;
        let y = if filter.to_ascii_lowercase().contains("send") && height > width * 3 {
            f64::from(element.bounds.top) + 12.0_f64.min(f64::from(height) / 2.0)
        } else {
            f64::from(element.bounds.top + element.bounds.bottom) / 2.0
        };
        self.tap(device_id, Point { x, y }).await
    }
}

#[async_trait]
impl Robot for IosSimulatorRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let stdout = self
            .simctl(&[
                "simctl".into(),
                "list".into(),
                "devices".into(),
                "available".into(),
                "-j".into(),
            ])
            .await?;
        parse_simctl_devices(&String::from_utf8(stdout).map_err(|error| {
            RobotError::CommandFailed(format!("simctl returned invalid UTF-8: {error}"))
        })?)
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        Self::valid_device_id(device_id)?;
        let apps = self
            .simctl_json(&["simctl".into(), "listapps".into(), device_id.into()])
            .await?;
        Ok(json!({
            "platform": "ios",
            "deviceId": device_id,
            "apps": apps,
        }))
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        // bootstatus -b is idempotent and waits for readiness even when another
        // caller has already started the simulator but it is still booting.
        tokio::time::timeout(
            Duration::from_secs(180),
            self.simctl(&[
                "simctl".into(),
                "bootstatus".into(),
                device_id.into(),
                "-b".into(),
            ]),
        ).await.map_err(|_| RobotError::CommandFailed(format!(
            "iOS Simulator {device_id} did not finish starting within 180 seconds; refresh devices before retrying"
        )))??;
        Ok(())
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Self::valid_device_id(device_id)?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create recording directory: {error}"))
            })?;

        let mut recordings = self.recordings.lock().await;
        if recordings.contains_key(device_id) {
            return Err(RobotError::InvalidInput(format!(
                "a screen recording is already running for device {device_id}"
            )));
        }
        let preflight = self.check_recording_session(device_id).await?;
        if preflight.check.status != robot_core::health::CheckStatus::Pass {
            return Err(RobotError::CommandFailed(format!(
                "recording preflight blocked: {}. Run device-bot --health {device_id}",
                serde_json::to_string(&preflight)
                    .unwrap_or_else(|_| preflight.check.detail.clone())
            )));
        }
        let path = self.next_recording_path(device_id);
        let child = Command::new(&self.xcrun_path)
            .args([
                "simctl",
                "io",
                device_id,
                "recordVideo",
                &path.to_string_lossy(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start simctl screen recording: {error}"
                ))
            })?;
        recordings.insert(
            device_id.into(),
            ActiveRecording {
                child,
                path: path.clone(),
            },
        );
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        Self::valid_device_id(device_id)?;
        let Some(mut recording) = self.recordings.lock().await.remove(device_id) else {
            return Err(RobotError::InvalidInput(format!(
                "no screen recording is running for device {device_id}"
            )));
        };
        #[cfg(unix)]
        if let Some(pid) = recording.child.id() {
            // `simctl io recordVideo` finalizes its MP4 when it receives SIGINT.
            unsafe { libc::kill(pid as i32, libc::SIGINT) };
        }
        #[cfg(not(unix))]
        recording.child.start_kill().map_err(|error| {
            RobotError::CommandFailed(format!("could not stop simctl screen recording: {error}"))
        })?;
        recording.child.wait().await.map_err(|error| {
            RobotError::CommandFailed(format!("could not stop simctl screen recording: {error}"))
        })?;
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        Self::valid_device_id(device_id)?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create screenshot directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "mobile-mcp-ios-{}-{}.png",
            std::process::id(),
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        self.simctl(&[
            "simctl".into(),
            "io".into(),
            device_id.into(),
            "screenshot".into(),
            path.to_string_lossy().into_owned(),
        ])
        .await?;
        let bytes = tokio::fs::read(&path).await.map_err(|error| {
            RobotError::CommandFailed(format!("could not read simulator screenshot: {error}"))
        })?;
        let _ = tokio::fs::remove_file(path).await;
        Ok(bytes)
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        Self::valid_id(app_id, "app id")?;
        self.simctl(&[
            "simctl".into(),
            "launch".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await?;
        self.active_apps
            .lock()
            .await
            .insert(device_id.into(), app_id.into());
        Ok(())
    }
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        self.simctl(&[
            "simctl".into(),
            "openurl".into(),
            device_id.into(),
            url.into(),
        ])
        .await?;
        self.active_apps.lock().await.insert(
            device_id.into(),
            app_id.unwrap_or("com.apple.mobilesafari").into(),
        );
        Ok(())
    }
    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        self.simctl(&[
            "simctl".into(),
            "install".into(),
            device_id.into(),
            path.into(),
        ])
        .await
        .map(|_| ())
    }
    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        self.simctl(&[
            "simctl".into(),
            "uninstall".into(),
            device_id.into(),
            app_id.into(),
        ])
        .await?;
        let mut active_apps = self.active_apps.lock().await;
        if active_apps
            .get(device_id)
            .is_some_and(|active| active == app_id)
        {
            active_apps.remove(device_id);
        }
        Ok(())
    }
    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        Self::valid_point(point, "tap")?;
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "tap",
            &[
                ("DEVICE_BOT_X", point.x.to_string()),
                ("DEVICE_BOT_Y", point.y.to_string()),
            ],
            None,
        )
        .await
        .map(|_| ())
    }
    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        if text.is_empty() {
            return Err(RobotError::InvalidInput("text must not be empty".into()));
        }
        if text.contains('\0') {
            return Err(RobotError::InvalidInput("text must not contain NUL".into()));
        }
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "type",
            &[("DEVICE_BOT_TEXT", text.into())],
            None,
        )
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
        Self::valid_point(start, "swipe start")?;
        Self::valid_point(end, "swipe end")?;
        let app_id = self.active_app(device_id).await?;
        self.run_xctest(
            device_id,
            &app_id,
            "swipe",
            &[
                ("DEVICE_BOT_X", start.x.to_string()),
                ("DEVICE_BOT_Y", start.y.to_string()),
                ("DEVICE_BOT_END_X", end.x.to_string()),
                ("DEVICE_BOT_END_Y", end.y.to_string()),
                (
                    "DEVICE_BOT_DURATION_SECONDS",
                    format!("{:.3}", duration_ms.unwrap_or(100) as f64 / 1_000.0),
                ),
            ],
            None,
        )
        .await
        .map(|_| ())
    }
    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_filtered(device_id, None, None).await
    }

    async fn execute_ui_actions(
        &self,
        device_id: &str,
        actions: &[UiAction],
    ) -> Result<(), RobotError> {
        if actions.is_empty() {
            return Err(RobotError::InvalidInput("actions must not be empty".into()));
        }
        let app_id = self.active_app(device_id).await?;
        for action in actions {
            match action {
                UiAction::Tap { x, y } => {
                    Self::valid_point(Point { x: *x, y: *y }, "tap")?;
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "tap",
                        &[
                            ("DEVICE_BOT_X", x.to_string()),
                            ("DEVICE_BOT_Y", y.to_string()),
                        ],
                        None,
                    )
                    .await?;
                }
                UiAction::TapElement { filter } => {
                    if filter.trim().is_empty() {
                        return Err(RobotError::InvalidInput(
                            "tap element filter must not be empty".into(),
                        ));
                    }
                    if filter.to_ascii_lowercase().contains("send") {
                        self.tap_element_by_filter(device_id, filter).await?;
                    } else {
                        let result = self
                            .run_xctest(
                                device_id,
                                &app_id,
                                "tap-element",
                                &[("DEVICE_BOT_ELEMENT_FILTER", filter.clone())],
                                None,
                            )
                            .await?;
                        if result.get("ok").and_then(Value::as_bool) == Some(false) {
                            return Err(RobotError::CommandFailed(format!(
                                "could not tap UI element matching {:?}",
                                filter
                            )));
                        }
                    }
                }
                UiAction::Type { text } => {
                    if text.is_empty() || text.contains('\0') {
                        return Err(RobotError::InvalidInput(
                            "text must be non-empty and must not contain NUL".into(),
                        ));
                    }
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "type",
                        &[("DEVICE_BOT_TEXT", text.clone())],
                        None,
                    )
                    .await?;
                }
                UiAction::Swipe {
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    duration_ms,
                } => {
                    Self::valid_point(
                        Point {
                            x: *start_x,
                            y: *start_y,
                        },
                        "swipe start",
                    )?;
                    Self::valid_point(
                        Point {
                            x: *end_x,
                            y: *end_y,
                        },
                        "swipe end",
                    )?;
                    self.run_xctest(
                        device_id,
                        &app_id,
                        "swipe",
                        &[
                            ("DEVICE_BOT_X", start_x.to_string()),
                            ("DEVICE_BOT_Y", start_y.to_string()),
                            ("DEVICE_BOT_END_X", end_x.to_string()),
                            ("DEVICE_BOT_END_Y", end_y.to_string()),
                            (
                                "DEVICE_BOT_DURATION_SECONDS",
                                format!("{:.3}", duration_ms.unwrap_or(100) as f64 / 1_000.0),
                            ),
                        ],
                        None,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        Self::valid_device_id(device_id)?;
        let derived = Self::xctest_derived_data();
        let cache = RunnerCache {
            project: &self.xctest_project,
            derived_data: &derived,
            simulator: true,
        };
        let expected = cache.version()?;
        let health = match &self.simulator_agent {
            Some(agent) => {
                agent
                    .dispatch_for(device_id, "health", json!({}), Duration::from_secs(2))
                    .await
            }
            None => Err(RobotError::Unsupported(
                "Simulator helper bridge is not configured".into(),
            )),
        };
        let status = live_status(
            device_id,
            Some(expected.clone()),
            cache.is_current(&expected),
            health,
        );
        if status.ready {
            self.active_apps
                .lock()
                .await
                .insert(device_id.into(), status.target_app_id.clone().unwrap());
        }
        Ok(status)
    }

    async fn prepare_ui_runner(
        &self,
        device_id: &str,
        app_id: Option<&str>,
        reinstall: bool,
    ) -> Result<UiRunnerStatus, RobotError> {
        Self::valid_device_id(device_id)?;
        let _lock = self.xctest_lock.lock().await;
        let booted = self
            .list_devices()
            .await?
            .iter()
            .any(|d| d.id == device_id && d.state == DeviceState::Available);
        if !booted {
            return Err(RobotError::InvalidInput(
                "Boot the simulator before preparing its helper".into(),
            ));
        }
        if let Some(app_id) = app_id {
            Self::valid_id(app_id, "app id")?;
            self.simctl(&[
                "simctl".into(),
                "get_app_container".into(),
                device_id.into(),
                app_id.into(),
                "app".into(),
            ])
            .await?;
        }
        // Recover the target from a helper left alive by an earlier MCP process.
        let recovered_target = self
            .ui_runner_status(device_id)
            .await
            .ok()
            .filter(|status| status.state != "wrong_device")
            .and_then(|status| status.target_app_id);
        if reinstall {
            self.stop_persistent_agent(device_id).await;
        }
        let target = app_id
            .map(str::to_owned)
            .or(self.active_apps.lock().await.get(device_id).cloned())
            .or(recovered_target)
            .unwrap_or_default();
        let derived = Self::xctest_derived_data();
        self.ensure_persistent_agent(
            device_id,
            &target,
            &self.xctest_project.to_string_lossy(),
            &format!("platform=iOS Simulator,id={device_id}"),
            &derived,
        )
        .await?;
        self.ui_runner_status(device_id).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        let app_id = self.active_app(device_id).await?;
        let mut values = Vec::new();
        if let Some(filter) = filter {
            values.push(("DEVICE_BOT_ELEMENT_FILTER", filter.to_owned()));
        }
        if let Some(max_results) = max_results {
            values.push(("DEVICE_BOT_MAX_RESULTS", max_results.to_string()));
        }
        let result = self
            .run_xctest(device_id, &app_id, "list-elements", &values, None)
            .await?;
        let mut elements = decode_agent_elements(&result)?;
        let filter = filter.map(str::to_ascii_lowercase);
        if let Some(filter) = filter.as_deref() {
            elements.retain(|element| robot_core::element_matches_filter(element, filter));
        }
        if let Some(max_results) = max_results {
            elements.truncate(max_results);
        }
        Ok(elements)
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
        let app_id = self.active_app(device_id).await?;
        let mut values = vec![("DEVICE_BOT_ELEMENT_TYPE", element_type.as_str().to_owned())];
        if let Some(filter) = query.filter.as_deref() {
            values.push(("DEVICE_BOT_ELEMENT_FILTER", filter.to_owned()));
        }
        if let Some(max_results) = query.max_results {
            values.push(("DEVICE_BOT_MAX_RESULTS", max_results.to_string()));
        }
        let result = self
            .run_xctest(device_id, &app_id, "list-elements", &values, None)
            .await?;
        if result.get("appliedElementType").and_then(Value::as_str) != Some(element_type.as_str()) {
            return Err(RobotError::Unsupported(
                "iOS Simulator XCTest agent did not apply elementType; rebuild and reinstall the runner before querying by type".into(),
            ));
        }
        let elements = decode_agent_elements(&result)?;
        Ok(apply_element_query(elements, &query))
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        let app_id = self.active_app(device_id).await?;
        match self
            .run_xctest(
                device_id,
                &app_id,
                "ime-action",
                &[("DEVICE_BOT_IME_ACTION", action.as_str().to_owned())],
                None,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(RobotError::CommandFailed(message)) if message.contains("unknown_method") => {
                Err(RobotError::Unsupported(
                    "iOS Simulator XCTest agent does not support IME actions; rebuild and reinstall the runner".into(),
                ))
            }
            Err(error) => Err(error),
        }
    }
}

fn decode_agent_elements(result: &Value) -> Result<Vec<UiElement>, RobotError> {
    let elements: Vec<AgentElement> = serde_json::from_value(
        result
            .get("elements")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .map_err(|error| {
        RobotError::CommandFailed(format!(
            "invalid persistent XCTest element payload: {error}"
        ))
    })?;
    Ok(elements
        .into_iter()
        .map(AgentElement::into_ui_element)
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentElement {
    node_id: String,
    automation_id: Option<String>,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    role: String,
    bounds: AgentBounds,
    enabled: bool,
    clickable: bool,
    #[serde(default)]
    focused: Option<bool>,
    #[serde(default)]
    checked: Option<bool>,
    #[serde(default)]
    selected: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl AgentElement {
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

#[derive(Deserialize)]
struct SimctlList {
    devices: HashMap<String, Vec<SimctlDevice>>,
}
#[derive(Deserialize)]
struct SimctlDevice {
    udid: String,
    name: String,
    state: String,
    #[serde(rename = "isAvailable")]
    available: Option<bool>,
}

pub fn parse_simctl_devices(json: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let list: SimctlList = serde_json::from_str(json).map_err(|error| {
        RobotError::CommandFailed(format!("could not parse simctl devices: {error}"))
    })?;
    Ok(list
        .devices
        .into_values()
        .flatten()
        .filter(|device| device.available.unwrap_or(true))
        .map(|device| RobotDevice {
            id: device.udid,
            name: device.name,
            platform: Platform::Ios,
            state: match device.state.as_str() {
                "Booted" => DeviceState::Available,
                "Shutdown" => DeviceState::Offline,
                _ => DeviceState::Unknown,
            },
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn start_waits_for_simctl_bootstatus_and_propagates_boot_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "device-bot-simulator-start-{}-{}",
            std::process::id(),
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("xcrun");
        std::fs::write(&script, "#!/bin/sh\n[ \"$1\" = simctl ] && [ \"$2\" = bootstatus ] && [ \"$4\" = -b ] || exit 7\n[ \"$3\" = failed-id ] && { echo 'boot failed' >&2; exit 9; }\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let robot = IosSimulatorRobot::new(script.to_string_lossy(), &dir);
        robot.boot_device("simulator-id").await.unwrap();
        let error = robot.boot_device("failed-id").await.unwrap_err();
        assert!(error.to_string().contains("boot failed"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_booted_and_shutdown_simulators() {
        let devices = parse_simctl_devices(r#"{"devices":{"runtime":[{"udid":"booted-id","name":"iPhone 16","state":"Booted","isAvailable":true},{"udid":"off-id","name":"iPhone SE","state":"Shutdown","isAvailable":true}]}}"#).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].state, DeviceState::Available);
        assert_eq!(devices[1].state, DeviceState::Offline);
    }

    #[test]
    fn recording_paths_are_unique_and_stay_in_the_artifact_directory() {
        let robot = IosSimulatorRobot::new("xcrun", "artifacts");
        let first = robot.next_recording_path("test-device");
        let second = robot.next_recording_path("test-device");
        assert!(first.starts_with("artifacts"));
        assert_eq!(
            first.extension().and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert_ne!(first, second);
    }

    #[test]
    fn resolves_relative_xctest_artifact_paths_to_absolute_paths() {
        let path = IosSimulatorRobot::absolute_path(Path::new("artifacts")).unwrap();

        assert!(path.is_absolute());
        assert!(path.ends_with("artifacts"));
    }

    #[test]
    fn decodes_xctest_accessibility_elements() {
        let element: AgentElement = serde_json::from_str(
            r#"{"nodeId":"ios-1-0","automationId":"save-button","text":"Save","label":"Save changes","value":null,"role":"button","bounds":{"x":20,"y":100,"width":80,"height":44},"clickable":true,"enabled":true}"#,
        )
        .unwrap();
        let element = element.into_ui_element();
        assert_eq!(element.class_name, "button");
        assert_eq!(element.resource_id.as_deref(), Some("save-button"));
        assert_eq!(element.content_description.as_deref(), Some("Save changes"));
        assert_eq!(
            (
                element.bounds.left,
                element.bounds.top,
                element.bounds.right,
                element.bounds.bottom,
            ),
            (20, 100, 100, 144)
        );
        assert!(element.clickable);
        assert!(element.enabled);
    }

    #[test]
    fn rejects_unsafe_simulator_ids_and_invalid_screen_points() {
        assert!(IosSimulatorRobot::valid_device_id("--help").is_err());
        assert!(
            IosSimulatorRobot::valid_point(
                Point {
                    x: f64::NAN,
                    y: 0.0
                },
                "tap"
            )
            .is_err()
        );
        assert!(IosSimulatorRobot::valid_point(Point { x: -1.0, y: 0.0 }, "tap").is_err());
        assert!(IosSimulatorRobot::valid_point(Point { x: 12.6, y: 24.2 }, "tap").is_ok());
    }

    #[test]
    fn xctest_runner_cache_requires_both_app_products() {
        let derived_data = std::env::temp_dir().join(format!(
            "mobile-mcp-ios-xctest-cache-test-{}",
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let products = derived_data.join("Build/Products/Debug-iphonesimulator");
        std::fs::create_dir_all(&products).unwrap();
        std::fs::write(
            derived_data.join("Build/Products/stale.xctestrun"),
            "stale marker",
        )
        .unwrap();

        assert!(!IosSimulatorRobot::xctest_runner_ready(&derived_data));
        std::fs::create_dir(products.join("MobileMcpSimulatorRunnerHost.app")).unwrap();
        assert!(!IosSimulatorRobot::xctest_runner_ready(&derived_data));
        std::fs::create_dir(products.join("MobileMcpSimulatorRunnerUITests-Runner.app")).unwrap();
        assert!(IosSimulatorRobot::xctest_runner_ready(&derived_data));

        std::fs::remove_dir_all(derived_data).unwrap();
    }
}

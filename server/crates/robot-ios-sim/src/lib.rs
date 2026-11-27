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

use robot_core::{
    ElementQuery, Point, Robot, RobotError, UiElement, WebContext, apply_element_query,
};
use robot_ios_real::{
    XCTestAgentBridge,
    runner_cache::{RunnerCache, compatible, live_status},
};
use serde_json::{Value, json};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::Mutex,
    time::{Instant, sleep},
};

mod adapter;
pub mod health;
mod protocol;

use protocol::decode_agent_elements;
pub use protocol::parse_simctl_devices;
mod live;
static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const XCTEST_BUILD_TIMEOUT: Duration = Duration::from_secs(90);
const XCTEST_RUNNER_BUNDLE_ID: &str = "com.mobilemcp.simulatorrunner.tests.xctrunner";

#[derive(Clone)]
pub struct IosSimulatorRobot {
    live_targets: Arc<Mutex<HashMap<String, live::LiveTarget>>>,
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
            live_targets: Arc::default(),
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
                "DEVICE_BOT_DURATION_MS" => Some(("durationMs", json!(value.parse::<u64>().ok()?))),
                "DEVICE_BOT_DURATION_SECONDS" => Some((
                    "durationMs",
                    json!(value.parse::<f64>().ok()?.mul_add(1000.0, 0.0) as u64),
                )),
                "DEVICE_BOT_TEXT" => Some(("text", json!(value))),
                "DEVICE_BOT_ELEMENT_FILTER" => Some(("filter", json!(value))),
                "DEVICE_BOT_ELEMENT_TYPE" => Some(("elementType", json!(value))),
                "DEVICE_BOT_EXCLUDE_KEYBOARD" => Some(("excludeKeyboard", json!(true))),
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
            "screen-geometry" => {
                agent
                    .dispatch_for(device_id, "screen-geometry", Value::Object(params), timeout)
                    .await
            }
            "tap" | "long_press" | "double_tap" | "clear-text" => {
                agent
                    .dispatch_for(device_id, action, Value::Object(params), timeout)
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

    async fn webview_elements(
        &self,
        device_id: &str,
        context: &WebContext,
        query: &ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        context.validate()?;
        let bundle_id = match context.app_id.as_deref().filter(|id| !id.is_empty()) {
            Some(bundle_id) => bundle_id.to_owned(),
            None => self.active_app(device_id).await?,
        };
        let elements =
            robot_ios_real::webview::read_dom(device_id, &bundle_id, "sim", context).await?;
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
        let bundle_id = match context.app_id.as_deref().filter(|id| !id.is_empty()) {
            Some(bundle_id) => bundle_id.to_owned(),
            None => self.active_app(device_id).await?,
        };
        robot_ios_real::webview::edit_dom(device_id, &bundle_id, "sim", context, action, text).await
    }
}

#[cfg(test)]
mod tests;

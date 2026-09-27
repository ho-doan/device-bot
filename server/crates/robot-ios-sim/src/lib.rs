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
use mobile_robot_core::{
    Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiAction, UiElement,
    UiRunnerStatus,
};
use serde::Deserialize;
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct IosSimulatorRobot {
    xcrun_path: String,
    xcodebuild_path: String,
    xctest_project: PathBuf,
    artifact_dir: PathBuf,
    recordings: Arc<Mutex<HashMap<String, ActiveRecording>>>,
    active_apps: Arc<Mutex<HashMap<String, String>>>,
    xctest_lock: Arc<Mutex<()>>,
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
        Self {
            xcrun_path: xcrun_path.into(),
            xcodebuild_path: xcodebuild_path.into(),
            xctest_project: xctest_project.into(),
            artifact_dir: artifact_dir.into(),
            recordings: Arc::default(),
            active_apps: Arc::default(),
            xctest_lock: Arc::default(),
        }
    }

    async fn simctl(&self, args: &[String]) -> Result<Vec<u8>, RobotError> {
        let output = Command::new(&self.xcrun_path)
            .args(args)
            .stdin(Stdio::null())
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
            "mobile-mcp-ios-recording-{device_id}-{}.mp4",
            ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
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
        self.active_apps
            .lock()
            .await
            .get(device_id)
            .cloned()
            .ok_or_else(|| {
                RobotError::InvalidInput(format!(
                    "no active app is known for simulator {device_id}; launch an app before using UI tools"
                ))
            })
    }

    async fn run_xctest(
        &self,
        device_id: &str,
        target_app_id: &str,
        action: &str,
        values: &[(&str, String)],
    ) -> Result<RunnerResult, RobotError> {
        Self::valid_device_id(device_id)?;
        if !self.xctest_project.is_dir() {
            return Err(RobotError::CommandFailed(format!(
                "bundled XCTest project was not found at {}",
                self.xctest_project.display()
            )));
        }
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest artifact directory: {error}"
                ))
            })?;
        let artifact_dir = Self::absolute_path(&self.artifact_dir)?;
        let sequence = ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let result_path =
            artifact_dir.join(format!("mobile-mcp-ios-xctest-{device_id}-{sequence}.json"));
        // Keep DerivedData independent from request artifacts so every MCP server
        // process can reuse the compiled XCTest runner. The mutex below serializes
        // access to this shared cache.
        let derived_data = std::env::temp_dir().join("mobile-mcp-ios-xctest-derived-data");
        let project = self.xctest_project.to_string_lossy().into_owned();
        let destination = format!("platform=iOS Simulator,id={device_id}");
        let derived_data = derived_data.to_string_lossy().into_owned();
        let runner_env = values
            .iter()
            .map(|(name, value)| (format!("TEST_RUNNER_{name}"), value.clone()))
            .collect::<Vec<_>>();
        // Xcode uses a shared DerivedData directory, so concurrent UI calls must not
        // build or test against it at the same time.
        let _xctest_lock = self.xctest_lock.lock().await;
        let mut command = Command::new(&self.xcodebuild_path);
        command
            .args([
                "test-without-building",
                "-project",
                &project,
                "-scheme",
                "MobileMcpSimulatorRunner",
                "-destination",
                &destination,
                "-only-testing:MobileMcpSimulatorRunnerUITests/MobileMcpSimulatorRunnerUITests/testDispatch",
                "-derivedDataPath",
                &derived_data,
            ])
            .env("TEST_RUNNER_MOBILE_MCP_RESULT_PATH", &result_path)
            .env("TEST_RUNNER_MOBILE_MCP_ACTION", action)
            .env("TEST_RUNNER_MOBILE_MCP_TARGET_APP_ID", target_app_id)
            .envs(runner_env)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(45), command.output())
            .await
            .map_err(|_| {
                RobotError::CommandFailed(
                    "XCTest runner timed out after 45 seconds; use filter and maxResults for web views"
                        .into(),
                )
            })?
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start bundled XCTest runner at {}: {error}",
                    self.xcodebuild_path
                ))
            })?;
        let result = tokio::fs::read(&result_path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RunnerResult>(&bytes).ok());
        let _ = tokio::fs::remove_file(&result_path).await;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let Some(result) = result else {
            return Err(RobotError::CommandFailed(format!(
                "XCTest runner did not return a result (status {}): {stderr}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".into(), |code| code.to_string())
            )));
        };
        if !output.status.success() || !result.ok {
            return Err(RobotError::CommandFailed(result.error.unwrap_or_else(
                || {
                    format!(
                        "XCTest runner failed with status {}: {stderr}",
                        output
                            .status
                            .code()
                            .map_or_else(|| "unknown".into(), |code| code.to_string())
                    )
                },
            )));
        }
        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerResult {
    ok: bool,
    error: Option<String>,
    #[serde(default)]
    elements: Vec<RunnerElement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerElement {
    class_name: String,
    resource_id: String,
    text: String,
    content_description: String,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    clickable: bool,
    enabled: bool,
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

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        Self::valid_device_id(device_id)?;
        if self
            .list_devices()
            .await?
            .iter()
            .any(|device| device.id == device_id && device.state == DeviceState::Available)
        {
            return Ok(());
        }
        self.simctl(&["simctl".into(), "boot".into(), device_id.into()])
            .await?;
        self.simctl(&[
            "simctl".into(),
            "bootstatus".into(),
            device_id.into(),
            "-b".into(),
        ])
        .await?;
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
        if let Some(pid) = recording.child.id() {
            // `simctl io recordVideo` finalizes its MP4 when it receives SIGINT.
            unsafe { libc::kill(pid as i32, libc::SIGINT) };
        }
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
                ("MOBILE_MCP_X", point.x.to_string()),
                ("MOBILE_MCP_Y", point.y.to_string()),
            ],
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
            &[("MOBILE_MCP_TEXT", text.into())],
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
                ("MOBILE_MCP_X", start.x.to_string()),
                ("MOBILE_MCP_Y", start.y.to_string()),
                ("MOBILE_MCP_END_X", end.x.to_string()),
                ("MOBILE_MCP_END_Y", end.y.to_string()),
                (
                    "MOBILE_MCP_DURATION_SECONDS",
                    format!("{:.3}", duration_ms.unwrap_or(100) as f64 / 1_000.0),
                ),
            ],
        )
        .await
        .map(|_| ())
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
        let batch = serde_json::to_string(actions)
            .map_err(|error| RobotError::InvalidInput(error.to_string()))?;
        self.run_xctest(device_id, &app_id, "batch", &[("MOBILE_MCP_BATCH", batch)])
            .await
            .map(|_| ())
    }

    async fn ui_runner_status(&self, device_id: &str) -> Result<UiRunnerStatus, RobotError> {
        Self::valid_device_id(device_id)?;
        let products =
            std::env::temp_dir().join("mobile-mcp-ios-xctest-derived-data/Build/Products");
        let ready = std::fs::read_dir(&products)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "xctestrun")
            });
        Ok(UiRunnerStatus {
            ready,
            detail: if ready {
                "XCTest cache ready for test-without-building".into()
            } else {
                "XCTest cache missing; prewarm before recording".into()
            },
        })
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
            values.push(("MOBILE_MCP_ELEMENT_FILTER", filter.to_owned()));
        }
        if let Some(max_results) = max_results {
            values.push(("MOBILE_MCP_MAX_RESULTS", max_results.to_string()));
        }
        let result = self
            .run_xctest(device_id, &app_id, "list-elements", &values)
            .await?;
        Ok(result
            .elements
            .into_iter()
            .map(|element| UiElement {
                class_name: element.class_name,
                resource_id: non_empty(element.resource_id),
                text: non_empty(element.text),
                content_description: non_empty(element.content_description),
                bounds: Bounds {
                    left: element.left,
                    top: element.top,
                    right: element.right,
                    bottom: element.bottom,
                },
                clickable: element.clickable,
                enabled: element.enabled,
            })
            .collect())
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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
        let result: RunnerResult = serde_json::from_str(
            r#"{"ok":true,"elements":[{"className":"Button","resourceId":"save-button","text":"Save","contentDescription":"Save changes","left":20,"top":100,"right":100,"bottom":144,"clickable":true,"enabled":true}]}"#,
        )
        .unwrap();

        assert!(result.ok);
        assert_eq!(result.elements.len(), 1);
        let element = &result.elements[0];
        assert_eq!(element.class_name, "Button");
        assert_eq!(element.resource_id, "save-button");
        assert_eq!(element.content_description, "Save changes");
        assert_eq!(
            (element.left, element.top, element.right, element.bottom),
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
}

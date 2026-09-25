//! Android implementation of the platform-neutral [`Robot`] contract.
//!
//! Process execution is isolated behind [`CommandRunner`], letting the ADB
//! parser and command construction be tested without an emulator or `adb`.

use std::process::Stdio;

use async_trait::async_trait;
use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError};
use tokio::process::Command;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs external commands for the Android driver.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError>;
}

/// Real process runner used outside tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        let output = Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start {executable}: {error}"))
            })?;

        Ok(CommandOutput {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// ADB-backed Android device driver.
#[derive(Debug, Clone)]
pub struct AndroidRobot<R = TokioCommandRunner> {
    adb_path: String,
    runner: R,
}

impl AndroidRobot<TokioCommandRunner> {
    pub fn new(adb_path: impl Into<String>) -> Self {
        Self::with_runner(adb_path, TokioCommandRunner)
    }
}

impl<R> AndroidRobot<R> {
    pub fn with_runner(adb_path: impl Into<String>, runner: R) -> Self {
        Self {
            adb_path: adb_path.into(),
            runner,
        }
    }

    pub fn adb_path(&self) -> &str {
        &self.adb_path
    }
}

#[async_trait]
impl<R: CommandRunner> Robot for AndroidRobot<R> {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let output = self.run_adb(&["devices".into(), "-l".into()]).await?;
        let text = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!("adb devices returned invalid UTF-8: {error}"))
        })?;
        Ok(parse_adb_devices(&text))
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "exec-out".into(),
                "screencap".into(),
                "-p".into(),
            ])
            .await?;
        if output.stdout.is_empty() {
            return Err(RobotError::CommandFailed(
                "adb screencap returned an empty image".into(),
            ));
        }
        Ok(output.stdout)
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let x = android_coordinate(point.x, "x")?;
        let y = android_coordinate(point.y, "y")?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "tap".into(),
            x.to_string(),
            y.to_string(),
        ])
        .await?;
        Ok(())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "monkey".into(),
            "-p".into(),
            app_id.into(),
            "-c".into(),
            "android.intent.category.LAUNCHER".into(),
            "1".into(),
        ])
        .await?;
        Ok(())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_web_url(url)?;
        if let Some(app_id) = app_id {
            ensure_app_id(app_id)?;
        }

        let mut args = vec![
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "am".into(),
            "start".into(),
            "-W".into(),
            "-a".into(),
            "android.intent.action.VIEW".into(),
            "-d".into(),
            url.into(),
        ];
        if let Some(app_id) = app_id {
            args.extend(["-p".into(), app_id.into()]);
        }
        self.run_adb(&args).await?;
        Ok(())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let encoded = encode_input_text(text)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "text".into(),
            encoded,
        ])
        .await?;
        Ok(())
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

fn android_coordinate(value: f64, axis: &str) -> Result<i32, RobotError> {
    if !value.is_finite() || !(i32::MIN as f64..=i32::MAX as f64).contains(&value) {
        return Err(RobotError::InvalidInput(format!(
            "{axis} must be a finite 32-bit coordinate"
        )));
    }
    Ok(value.round() as i32)
}

/// Parses the output from `adb devices -l`.
pub fn parse_adb_devices(output: &str) -> Vec<RobotDevice> {
    output.lines().filter_map(parse_device_line).collect()
}

fn parse_device_line(line: &str) -> Option<RobotDevice> {
    let mut fields = line.split_whitespace();
    let id = fields.next()?;
    let state = fields.next()?;
    if id == "List" || id.starts_with('*') {
        return None;
    }

    let attributes: Vec<_> = fields.collect();
    let name = attributes
        .iter()
        .find_map(|field| field.strip_prefix("model:"))
        .map(|model| model.replace('_', " "))
        .unwrap_or_else(|| id.to_owned());

    Some(RobotDevice {
        id: id.to_owned(),
        name,
        platform: Platform::Android,
        state: match state {
            "device" => DeviceState::Available,
            "offline" => DeviceState::Offline,
            "unauthorized" => DeviceState::Unauthorized,
            _ => DeviceState::Unknown,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use super::*;

    #[derive(Debug)]
    struct FakeRunner {
        outputs: Mutex<VecDeque<CommandOutput>>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeRunner {
        fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().collect()),
                calls: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            executable: &str,
            args: &[String],
        ) -> Result<CommandOutput, RobotError> {
            self.calls
                .lock()
                .unwrap()
                .push((executable.to_owned(), args.to_vec()));
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| RobotError::CommandFailed("fake queue exhausted".into()))
        }
    }

    fn successful(stdout: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            success: true,
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: vec![],
        }
    }

    #[test]
    fn parses_all_adb_device_states() {
        let devices = parse_adb_devices(
            "List of devices attached\n\
             emulator-5554 device product:sdk_gphone64_arm64 model:Pixel_9 device:emu64a\n\
             R5CT30ABCD unauthorized transport_id:1\n\
             tcp:192.168.1.2:5555 offline product:foo\n",
        );

        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "Pixel 9");
        assert_eq!(devices[0].state, DeviceState::Available);
        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert_eq!(devices[2].state, DeviceState::Offline);
    }

    #[tokio::test]
    async fn uses_exec_out_for_screenshots_and_shell_input_for_taps() {
        let runner = FakeRunner::new([successful(b"png".to_vec()), successful(vec![])]);
        let robot = AndroidRobot::with_runner("adb-test", runner);

        assert_eq!(
            robot.take_screenshot("emulator-5554").await.unwrap(),
            b"png"
        );
        robot
            .tap("emulator-5554", Point { x: 12.5, y: 99.6 })
            .await
            .unwrap();

        assert_eq!(
            robot.runner.calls.lock().unwrap().as_slice(),
            [
                (
                    "adb-test".to_owned(),
                    vec!["-s", "emulator-5554", "exec-out", "screencap", "-p"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                ),
                (
                    "adb-test".to_owned(),
                    vec!["-s", "emulator-5554", "shell", "input", "tap", "13", "100"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                ),
            ]
        );
    }

    #[tokio::test]
    async fn lists_devices_through_adb() {
        let runner = FakeRunner::new([successful(
            "List of devices attached\nemulator-5554 device model:Pixel_9\n",
        )]);
        let robot = AndroidRobot::with_runner("adb", runner);

        let devices = robot.list_devices().await.unwrap();
        assert_eq!(devices[0].id, "emulator-5554");
        assert_eq!(
            robot.runner.calls.lock().unwrap()[0].1,
            vec!["devices", "-l"]
        );
    }

    #[tokio::test]
    async fn uses_explicit_adb_commands_for_app_navigation_and_text() {
        let runner = FakeRunner::new([successful(vec![]), successful(vec![]), successful(vec![])]);
        let robot = AndroidRobot::with_runner("adb-test", runner);

        robot
            .launch_app("emulator-5554", "com.android.chrome")
            .await
            .unwrap();
        robot
            .open_url(
                "emulator-5554",
                "https://www.youtube.com/results?search_query=babyshake",
                Some("com.android.chrome"),
            )
            .await
            .unwrap();
        robot
            .type_text("emulator-5554", "baby shake")
            .await
            .unwrap();

        assert_eq!(
            robot.runner.calls.lock().unwrap().as_slice(),
            [
                (
                    "adb-test".to_owned(),
                    vec![
                        "-s",
                        "emulator-5554",
                        "shell",
                        "monkey",
                        "-p",
                        "com.android.chrome",
                        "-c",
                        "android.intent.category.LAUNCHER",
                        "1"
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                ),
                (
                    "adb-test".to_owned(),
                    vec![
                        "-s",
                        "emulator-5554",
                        "shell",
                        "am",
                        "start",
                        "-W",
                        "-a",
                        "android.intent.action.VIEW",
                        "-d",
                        "https://www.youtube.com/results?search_query=babyshake",
                        "-p",
                        "com.android.chrome"
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                ),
                (
                    "adb-test".to_owned(),
                    vec![
                        "-s",
                        "emulator-5554",
                        "shell",
                        "input",
                        "text",
                        "baby%sshake"
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                ),
            ]
        );
    }
}

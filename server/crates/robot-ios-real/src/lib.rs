//! Real iOS device driver backed by `go-ios` and its WDA/DeviceKit UI runner.
//!
//! `IosRealRobot` retains the legacy `go-ios` adapter. `IosXCTestRobot` is the
//! primary server-side real-device implementation and uses Apple CoreDevice plus
//! a signed XCTest runner kept in this crate.

mod quicktime_recording;
pub mod xctest_agent;
pub mod xctest_robot;

pub use xctest_agent::XCTestAgentBridge;
pub use xctest_robot::{IosXCTestRobot, XCTestAgentLauncher};

pub mod health;

/// Finds the team ID belonging to the requested (or only) local Apple
/// Development signing identity. Xcode still needs a valid login for that team
/// to create provisioning profiles, but callers no longer need to copy its ID
/// into build commands by hand.
pub async fn apple_development_team(identity_hint: Option<&str>) -> Result<String, RobotError> {
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .await
        .map_err(|error| {
            RobotError::CommandFailed(format!("could not read signing identities: {error}"))
        })?;
    if !output.status.success() {
        return Err(RobotError::CommandFailed(format!(
            "security could not list signing identities: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let identities = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("Apple Development:"))
        .filter(|line| identity_hint.is_none_or(|hint| line.contains(hint)))
        .filter_map(|line| {
            let team = line
                .rsplit_once('(')?
                .1
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric())
                .collect::<String>();
            (team.len() == 10).then_some(team)
        })
        .collect::<Vec<_>>();
    match identities.as_slice() {
        [team] => Ok(team.clone()),
        [] => Err(RobotError::CommandFailed(match identity_hint {
            Some(hint) => format!("no Apple Development identity matched {hint:?}"),
            None => "no Apple Development signing identity was found in Keychain".into(),
        })),
        _ => Err(RobotError::CommandFailed(
            "multiple Apple Development identities found; set DEVICE_BOT_IOS_TEAM or DEVICE_BOT_IOS_SIGNING_IDENTITY".into(),
        )),
    }
}

/// Builds and provisions the server-owned XCTest runner for a connected iPhone.
/// When the only failure is Apple-account provisioning, opens Xcode so the user
/// can select or reauthenticate the intended account rather than guessing one.
pub async fn provision_xctest_runner(
    project: &std::path::Path,
    device_id: &str,
    development_team: Option<&str>,
    identity_hint: Option<&str>,
) -> Result<(), RobotError> {
    if !project.is_dir() {
        return Err(RobotError::CommandFailed(format!(
            "iOS XCTest project was not found at {}",
            project.display()
        )));
    }
    let project_team = tokio::fs::read_to_string(project.join("project.pbxproj"))
        .await
        .ok()
        .and_then(|contents| parse_xcode_project_team(&contents));
    let team = match development_team.filter(|team| !team.is_empty()) {
        Some(team) => team.to_owned(),
        None => match project_team {
            Some(team) => team,
            None => apple_development_team(identity_hint).await?,
        },
    };
    let derived_data = project
        .parent()
        .ok_or_else(|| {
            RobotError::CommandFailed("iOS XCTest project has no parent directory".into())
        })?
        .join("build");
    let output = Command::new("xcodebuild")
        .args(["build-for-testing", "-allowProvisioningUpdates", "-project"])
        .arg(project)
        .args([
            "-scheme",
            "MobileMcpRealRunner",
            "-destination",
            &format!("platform=iOS,id={device_id}"),
            "-derivedDataPath",
        ])
        .arg(derived_data)
        .arg(format!("DEVELOPMENT_TEAM={team}"))
        .output()
        .await
        .map_err(|error| {
            RobotError::CommandFailed(format!("could not start xcodebuild: {error}"))
        })?;
    if output.status.success() {
        return Ok(());
    }

    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let account_blocked = [
        "No Account for Team",
        "No profiles for",
        "No signing certificate",
        "Unable to log in with account",
        "Invalid credentials in keychain",
    ]
    .iter()
    .any(|needle| diagnostic.contains(needle));
    if account_blocked {
        let opened = Command::new("open")
            .args(["-a", "Xcode"])
            .arg(project)
            .spawn()
            .is_ok();
        let action = if opened {
            " Xcode was opened so you can select or reauthenticate the Apple account."
        } else {
            " Open the project in Xcode and select or reauthenticate the Apple account."
        };
        return Err(RobotError::CommandFailed(format!(
            "automatic iOS provisioning failed for team {team}.{action}\n{}",
            diagnostic.trim()
        )));
    }
    Err(RobotError::CommandFailed(format!(
        "iOS XCTest build failed:\n{}",
        diagnostic.trim()
    )))
}

fn parse_xcode_project_team(contents: &str) -> Option<String> {
    let teams = contents
        .lines()
        .filter_map(|line| line.trim().strip_prefix("DEVELOPMENT_TEAM = "))
        .filter_map(|value| value.strip_suffix(';'))
        .map(str::trim)
        .filter(|team| {
            team.len() == 10
                && team
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
        .collect::<std::collections::HashSet<_>>();
    (teams.len() == 1).then(|| teams.into_iter().next().unwrap().to_owned())
}

use std::{collections::HashMap, process::Stdio};

use async_trait::async_trait;
use quick_xml::{Reader, events::Event};
use robot_core::{Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use serde_json::Value;
use tokio::process::Command;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError>;
}

#[derive(Debug, Clone, Copy, Default)]
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

/// `go-ios` driver for a USB- or tunnel-connected real device.
#[derive(Debug, Clone)]
pub struct IosRealRobot<R = TokioCommandRunner> {
    ios_path: String,
    runner: R,
}

impl IosRealRobot<TokioCommandRunner> {
    pub fn new(ios_path: impl Into<String>) -> Self {
        Self::with_runner(ios_path, TokioCommandRunner)
    }
}

impl<R> IosRealRobot<R> {
    pub fn with_runner(ios_path: impl Into<String>, runner: R) -> Self {
        Self {
            ios_path: ios_path.into(),
            runner,
        }
    }
}

impl<R: CommandRunner> IosRealRobot<R> {
    async fn go_ios(&self, args: Vec<String>) -> Result<Vec<u8>, RobotError> {
        let output = self.runner.run(&self.ios_path, &args).await?;
        if output.success {
            return Ok(output.stdout);
        }
        Err(RobotError::CommandFailed(format!(
            "go-ios exited with status {}: {}",
            output
                .exit_code
                .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    fn device_args(prefix: &[&str], device_id: &str) -> Result<Vec<String>, RobotError> {
        validate_device_id(device_id)?;
        Ok(prefix
            .iter()
            .map(|value| (*value).to_owned())
            .chain(std::iter::once(format!("--udid={device_id}")))
            .collect())
    }
}

#[async_trait]
impl<R: CommandRunner> Robot for IosRealRobot<R> {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let output = self.go_ios(vec!["list".into(), "--details".into()]).await?;
        parse_go_ios_devices(&String::from_utf8(output).map_err(|error| {
            RobotError::CommandFailed(format!("go-ios returned invalid UTF-8: {error}"))
        })?)
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        let image = self
            .go_ios(Self::device_args(&["ui", "screenshot"], device_id)?)
            .await?;
        if image.starts_with(b"\x89PNG\r\n\x1a\n") {
            Ok(image)
        } else {
            Err(RobotError::CommandFailed(
                "go-ios UI runner returned a non-PNG screenshot; start DeviceKit or WDA first"
                    .into(),
            ))
        }
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        let (x, y) = screen_coordinates(point)?;
        let mut args = Self::device_args(&["ui", "tap"], device_id)?;
        args.extend([format!("--x={x}"), format!("--y={y}")]);
        self.go_ios(args).await.map(|_| ())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        validate_app_id(app_id)?;
        let mut args = Self::device_args(&["ui", "app", "launch"], device_id)?;
        args.push(app_id.into());
        self.go_ios(args).await.map(|_| ())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        let parsed = Url::parse(url).map_err(|_| RobotError::InvalidInput("invalid URL".into()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(RobotError::InvalidInput(
                "URL scheme must be http or https".into(),
            ));
        }
        let mut args = Self::device_args(&["webinspector", "launch", url], device_id)?;
        if let Some(app_id) = app_id {
            validate_app_id(app_id)?;
            args.push(format!("--bundle-id={app_id}"));
        }
        self.go_ios(args).await.map(|_| ())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        if text.is_empty() {
            return Err(RobotError::InvalidInput("text must not be empty".into()));
        }
        let mut args = Self::device_args(&["ui", "type"], device_id)?;
        args.push(format!("--text={text}"));
        self.go_ios(args).await.map(|_| ())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        _: Option<u64>,
    ) -> Result<(), RobotError> {
        let (start_x, start_y) = screen_coordinates(start)?;
        let (end_x, end_y) = screen_coordinates(end)?;
        let mut args = Self::device_args(&["ui", "swipe"], device_id)?;
        args.extend([
            format!("--from-x={start_x}"),
            format!("--from-y={start_y}"),
            format!("--to-x={end_x}"),
            format!("--to-y={end_y}"),
        ]);
        self.go_ios(args).await.map(|_| ())
    }

    async fn install_app(&self, device_id: &str, path: &str) -> Result<(), RobotError> {
        if path.trim().is_empty() {
            return Err(RobotError::InvalidInput(
                "app path must not be empty".into(),
            ));
        }
        let mut args = Self::device_args(&["install"], device_id)?;
        args.push(format!("--path={path}"));
        self.go_ios(args).await.map(|_| ())
    }

    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "go-ios does not expose a stable uninstall command; parity pass will add it".into(),
        ))
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        let output = self
            .go_ios(Self::device_args(&["ui", "source"], device_id)?)
            .await?;
        parse_wda_source(&String::from_utf8(output).map_err(|error| {
            RobotError::CommandFailed(format!("go-ios returned invalid UI XML: {error}"))
        })?)
    }
}

pub fn parse_go_ios_devices(json: &str) -> Result<Vec<RobotDevice>, RobotError> {
    let value: Value = serde_json::from_str(json).map_err(|error| {
        RobotError::CommandFailed(format!("could not parse go-ios device list: {error}"))
    })?;
    let entries = value
        .get("deviceList")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .ok_or_else(|| RobotError::CommandFailed("go-ios device list was not an array".into()))?;
    Ok(entries
        .iter()
        .filter_map(|entry| match entry {
            Value::String(id) if !id.is_empty() => Some(RobotDevice {
                id: id.clone(),
                name: "iOS device".into(),
                platform: Platform::Ios,
                state: DeviceState::Available,
            }),
            Value::Object(details) => details
                .get("UniqueDeviceID")
                .or_else(|| details.get("udid"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(|id| RobotDevice {
                    id: id.into(),
                    name: details
                        .get("DeviceName")
                        .or_else(|| details.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("iOS device")
                        .into(),
                    platform: Platform::Ios,
                    state: DeviceState::Available,
                }),
            _ => None,
        })
        .collect())
}

pub fn parse_wda_source(xml: &str) -> Result<Vec<UiElement>, RobotError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut elements = vec![];
    loop {
        match reader.read_event() {
            Ok(Event::Start(node)) | Ok(Event::Empty(node)) => {
                let attributes: HashMap<_, _> = node
                    .attributes()
                    .filter_map(Result::ok)
                    .filter_map(|attribute| {
                        let key = std::str::from_utf8(attribute.key.as_ref()).ok()?;
                        let value = attribute.unescape_value().ok()?.into_owned();
                        Some((key.to_owned(), value))
                    })
                    .collect();
                if let Some(element) = wda_element(&attributes) {
                    elements.push(element);
                }
            }
            Ok(Event::Eof) => return Ok(elements),
            Err(error) => {
                return Err(RobotError::CommandFailed(format!(
                    "could not parse WDA UI source: {error}"
                )));
            }
            _ => {}
        }
    }
}

fn wda_element(attributes: &HashMap<String, String>) -> Option<UiElement> {
    let class_name = attributes.get("type")?.clone();
    let bounds = Bounds {
        left: attributes.get("x")?.parse().ok()?,
        top: attributes.get("y")?.parse().ok()?,
        right: attributes.get("x")?.parse::<i32>().ok()?
            + attributes.get("width")?.parse::<i32>().ok()?,
        bottom: attributes.get("y")?.parse::<i32>().ok()?
            + attributes.get("height")?.parse::<i32>().ok()?,
    };
    Some(UiElement {
        class_name,
        resource_id: attributes
            .get("name")
            .filter(|value| !value.is_empty())
            .cloned(),
        text: attributes
            .get("label")
            .filter(|value| !value.is_empty())
            .cloned(),
        content_description: attributes
            .get("value")
            .filter(|value| !value.is_empty())
            .cloned(),
        bounds,
        clickable: attributes
            .get("enabled")
            .is_some_and(|value| value == "true"),
        enabled: attributes
            .get("enabled")
            .is_none_or(|value| value == "true"),
        focused: optional_bool_attr(attributes, "focused"),
        checked: optional_bool_attr(attributes, "checked"),
        selected: optional_bool_attr(attributes, "selected"),
        value: attributes
            .get("value")
            .filter(|value| !value.is_empty())
            .cloned(),
        element_type: None,
    })
}

fn optional_bool_attr(attributes: &HashMap<String, String>, key: &str) -> Option<bool> {
    match attributes.get(key).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

fn validate_device_id(device_id: &str) -> Result<(), RobotError> {
    if device_id.is_empty()
        || !device_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(RobotError::InvalidInput("invalid iOS device id".into()));
    }
    Ok(())
}

fn validate_app_id(app_id: &str) -> Result<(), RobotError> {
    if app_id.len() < 3
        || !app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return Err(RobotError::InvalidInput("invalid app id".into()));
    }
    Ok(())
}

fn screen_coordinates(point: Point) -> Result<(i64, i64), RobotError> {
    if !point.x.is_finite() || !point.y.is_finite() || point.x < 0.0 || point.y < 0.0 {
        return Err(RobotError::InvalidInput(
            "screen coordinates must be finite and non-negative".into(),
        ));
    }
    Ok((point.x.round() as i64, point.y.round() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_detailed_and_compact_go_ios_device_lists() {
        let detailed = parse_go_ios_devices(
            r#"{"deviceList":[{"UniqueDeviceID":"real-id","DeviceName":"Alice iPhone"}]}"#,
        )
        .unwrap();
        assert_eq!(detailed[0].id, "real-id");
        assert_eq!(detailed[0].name, "Alice iPhone");
        let compact = parse_go_ios_devices(r#"{"deviceList":["second-id"]}"#).unwrap();
        assert_eq!(compact[0].id, "second-id");
    }

    #[test]
    fn normalizes_wda_xml_to_portable_elements() {
        let elements = parse_wda_source(r#"<App><XCUIElementTypeButton type="XCUIElementTypeButton" name="save" label="Save" value="" enabled="true" x="10" y="20" width="30" height="40"/></App>"#).unwrap();
        assert_eq!(elements[0].class_name, "XCUIElementTypeButton");
        assert_eq!(elements[0].bounds.right, 40);
        assert_eq!(elements[0].text.as_deref(), Some("Save"));
    }
}

//! YAML-driven workflow orchestration for QC and AI-authored device scenarios.
//!
//! Flow-specific concepts intentionally live here instead of `robot-core`.
//! `robot-core` remains the platform-neutral device boundary; this crate owns
//! step sequencing, device selection, element queries, waits, and artifacts.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, UiElement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

const FLOW_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FlowDocument {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FlowStep {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub select_device: Option<SelectDeviceStep>,
    #[serde(default)]
    pub wait: Option<WaitStep>,
    #[serde(default)]
    pub launch_app: Option<LaunchAppStep>,
    #[serde(default)]
    pub open_url: Option<OpenUrlStep>,
    #[serde(default)]
    pub start_screen_recording: Option<EmptyStep>,
    #[serde(default)]
    pub stop_screen_recording: Option<EmptyStep>,
    #[serde(default)]
    pub screenshot: Option<ScreenshotStep>,
    #[serde(default)]
    pub tap: Option<TapStep>,
    #[serde(default)]
    pub tap_element: Option<ElementQueryStep>,
    #[serde(default)]
    pub type_text: Option<TypeTextStep>,
    #[serde(default)]
    pub swipe: Option<SwipeStep>,
    #[serde(default)]
    pub assert_element: Option<ElementQueryStep>,
    #[serde(default)]
    pub list_elements: Option<ElementQueryStep>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct EmptyStep {}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SelectDeviceStep {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub platform: Option<Platform>,
    #[serde(default)]
    pub state: Option<DeviceState>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WaitStep {
    pub ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LaunchAppStep {
    pub app_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct OpenUrlStep {
    pub url: String,
    #[serde(default)]
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ScreenshotStep {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TapStep {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ElementQueryStep {
    pub filter: String,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TypeTextStep {
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SwipeStep {
    pub start: Point,
    pub end: Point,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct FlowContext {
    pub selected_device: Option<RobotDevice>,
    pub variables: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FlowReport {
    pub name: String,
    pub dry_run: bool,
    pub selected_device: Option<RobotDevice>,
    pub steps: Vec<StepReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepReport {
    pub index: usize,
    pub id: String,
    pub action: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("could not read flow {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse flow YAML: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("flow validation failed: {0}")]
    Validation(String),
    #[error("step {index} ({id}) failed: {message}")]
    Step {
        index: usize,
        id: String,
        message: String,
    },
    #[error("could not write screenshot {path}: {source}")]
    Artifact {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl FlowDocument {
    pub fn from_yaml(source: &str) -> Result<Self, FlowError> {
        let flow: Self = serde_yaml::from_str(source)?;
        flow.validate()?;
        Ok(flow)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FlowError> {
        let path = path.as_ref().to_path_buf();
        let source = fs::read_to_string(&path).map_err(|source| FlowError::Read {
            path: path.clone(),
            source,
        })?;
        Self::from_yaml(&source)
    }

    pub fn validate(&self) -> Result<(), FlowError> {
        if self.version != FLOW_VERSION {
            return Err(FlowError::Validation(format!(
                "unsupported version {}; expected {FLOW_VERSION}",
                self.version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(FlowError::Validation("name must not be empty".into()));
        }
        if self.steps.is_empty() {
            return Err(FlowError::Validation("steps must not be empty".into()));
        }
        for (index, step) in self.steps.iter().enumerate() {
            let action_count = [
                step.select_device.is_some(),
                step.wait.is_some(),
                step.launch_app.is_some(),
                step.open_url.is_some(),
                step.start_screen_recording.is_some(),
                step.stop_screen_recording.is_some(),
                step.screenshot.is_some(),
                step.tap.is_some(),
                step.tap_element.is_some(),
                step.type_text.is_some(),
                step.swipe.is_some(),
                step.assert_element.is_some(),
                step.list_elements.is_some(),
            ]
            .into_iter()
            .filter(|present| *present)
            .count();
            if action_count != 1 {
                return Err(FlowError::Validation(format!(
                    "step {} must contain exactly one action",
                    step_id(index, step)
                )));
            }
            if let Some(wait) = &step.wait {
                if wait.ms > 600_000 {
                    return Err(FlowError::Validation(format!(
                        "step {} wait.ms must be <= 600000",
                        step_id(index, step)
                    )));
                }
            }
            for query in [&step.tap_element, &step.assert_element, &step.list_elements]
                .into_iter()
                .flatten()
            {
                if query.filter.trim().is_empty() {
                    return Err(FlowError::Validation(format!(
                        "step {} filter must not be empty",
                        step_id(index, step)
                    )));
                }
                if query
                    .max_results
                    .is_some_and(|value| !(1..=100).contains(&value))
                {
                    return Err(FlowError::Validation(format!(
                        "step {} max_results must be between 1 and 100",
                        step_id(index, step)
                    )));
                }
            }
        }
        Ok(())
    }
}

pub struct FlowRunner {
    robot: Option<Arc<dyn Robot>>,
    dry_run: bool,
}

impl FlowRunner {
    pub fn with_robot(robot: Arc<dyn Robot>) -> Self {
        Self {
            robot: Some(robot),
            dry_run: false,
        }
    }

    pub fn dry_run() -> Self {
        Self {
            robot: None,
            dry_run: true,
        }
    }

    pub async fn run(&self, flow: &FlowDocument) -> Result<FlowReport, FlowError> {
        flow.validate()?;
        let mut context = FlowContext::default();
        let mut reports = Vec::with_capacity(flow.steps.len());
        for (index, step) in flow.steps.iter().enumerate() {
            let id = step_id(index, step);
            let (action, output) = self.run_step(index, &id, step, &mut context).await?;
            reports.push(StepReport {
                index,
                id,
                action,
                status: "passed",
                output,
            });
        }
        Ok(FlowReport {
            name: flow.name.clone(),
            dry_run: self.dry_run,
            selected_device: context.selected_device,
            steps: reports,
        })
    }

    async fn run_step(
        &self,
        index: usize,
        id: &str,
        step: &FlowStep,
        context: &mut FlowContext,
    ) -> Result<(String, Option<Value>), FlowError> {
        if let Some(selection) = &step.select_device {
            let selected = if self.dry_run {
                RobotDevice {
                    id: selection
                        .id
                        .clone()
                        .unwrap_or_else(|| "<device-selected-at-runtime>".into()),
                    name: selection.name.clone().unwrap_or_default(),
                    platform: selection.platform.clone().unwrap_or(Platform::Macos),
                    state: selection.state.clone().unwrap_or(DeviceState::Available),
                }
            } else {
                let devices = self
                    .robot(index, id)?
                    .list_devices()
                    .await
                    .map_err(|error| step_error(index, id, error))?;
                select_device(selection, &devices)
                    .map_err(|message| step_error(index, id, message))?
            };
            context.selected_device = Some(selected.clone());
            return Ok(("select_device".into(), Some(json!(selected))));
        }

        if let Some(wait) = &step.wait {
            if !self.dry_run {
                tokio::time::sleep(Duration::from_millis(wait.ms)).await;
            }
            return Ok(("wait".into(), Some(json!({ "ms": wait.ms }))));
        }

        if let Some(launch) = &step.launch_app {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                self.robot(index, id)?
                    .launch_app(device_id, &launch.app_id)
                    .await
                    .map_err(|error| step_error(index, id, error))?;
            }
            return Ok((
                "launch_app".into(),
                Some(json!({ "device_id": device_id, "app_id": launch.app_id })),
            ));
        }

        if let Some(open) = &step.open_url {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                self.robot(index, id)?
                    .open_url(device_id, &open.url, open.app_id.as_deref())
                    .await
                    .map_err(|error| step_error(index, id, error))?;
            }
            return Ok((
                "open_url".into(),
                Some(json!({
                    "device_id": device_id,
                    "url": open.url,
                    "app_id": open.app_id,
                })),
            ));
        }

        if step.start_screen_recording.is_some() {
            let device_id = selected_device_id(index, id, context)?;
            let output = if self.dry_run {
                None
            } else {
                Some(
                    self.robot(index, id)?
                        .start_screen_recording(device_id)
                        .await
                        .map_err(|error| step_error(index, id, error))?,
                )
            };
            return Ok((
                "start_screen_recording".into(),
                output.map(|path| json!({ "path": path })),
            ));
        }

        if step.stop_screen_recording.is_some() {
            let device_id = selected_device_id(index, id, context)?;
            let output = if self.dry_run {
                None
            } else {
                Some(
                    self.robot(index, id)?
                        .stop_screen_recording(device_id)
                        .await
                        .map_err(|error| step_error(index, id, error))?,
                )
            };
            return Ok((
                "stop_screen_recording".into(),
                output.map(|path| json!({ "path": path })),
            ));
        }

        if let Some(screenshot) = &step.screenshot {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                let bytes = self
                    .robot(index, id)?
                    .take_screenshot(device_id)
                    .await
                    .map_err(|error| step_error(index, id, error))?;
                if let Some(parent) = screenshot
                    .path
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent).map_err(|source| FlowError::Artifact {
                        path: screenshot.path.clone(),
                        source,
                    })?;
                }
                fs::write(&screenshot.path, bytes).map_err(|source| FlowError::Artifact {
                    path: screenshot.path.clone(),
                    source,
                })?;
            }
            return Ok((
                "screenshot".into(),
                Some(json!({ "device_id": device_id, "path": screenshot.path })),
            ));
        }

        if let Some(tap) = &step.tap {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                self.robot(index, id)?
                    .tap(device_id, Point { x: tap.x, y: tap.y })
                    .await
                    .map_err(|error| step_error(index, id, error))?;
            }
            return Ok(("tap".into(), Some(json!({ "x": tap.x, "y": tap.y }))));
        }

        if let Some(query) = &step.tap_element {
            let device_id = selected_device_id(index, id, context)?;
            if self.dry_run {
                return Ok((
                    "tap_element".into(),
                    Some(json!({ "filter": query.filter, "max_results": query.max_results })),
                ));
            }
            let elements = self
                .robot(index, id)?
                .list_elements_filtered(device_id, Some(&query.filter), query.max_results)
                .await
                .map_err(|error| step_error(index, id, error))?;
            let element = elements
                .iter()
                .find(|element| element.enabled && element.clickable)
                .or_else(|| elements.iter().find(|element| element.enabled))
                .ok_or_else(|| step_error(index, id, "no enabled matching element"))?;
            let point = center(element);
            self.robot(index, id)?
                .tap(device_id, point)
                .await
                .map_err(|error| step_error(index, id, error))?;
            return Ok((
                "tap_element".into(),
                Some(json!({ "filter": query.filter, "x": point.x, "y": point.y })),
            ));
        }

        if let Some(text) = &step.type_text {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                self.robot(index, id)?
                    .type_text(device_id, &text.text)
                    .await
                    .map_err(|error| step_error(index, id, error))?;
            }
            return Ok(("type_text".into(), Some(json!({ "text": text.text }))));
        }

        if let Some(swipe) = &step.swipe {
            let device_id = selected_device_id(index, id, context)?;
            if !self.dry_run {
                self.robot(index, id)?
                    .swipe(device_id, swipe.start, swipe.end, swipe.duration_ms)
                    .await
                    .map_err(|error| step_error(index, id, error))?;
            }
            return Ok(("swipe".into(), Some(json!(swipe))));
        }

        if let Some(query) = &step.assert_element {
            let device_id = selected_device_id(index, id, context)?;
            if self.dry_run {
                return Ok((
                    "assert_element".into(),
                    Some(json!({ "filter": query.filter, "max_results": query.max_results })),
                ));
            }
            let elements = self
                .robot(index, id)?
                .list_elements_filtered(device_id, Some(&query.filter), query.max_results)
                .await
                .map_err(|error| step_error(index, id, error))?;
            if elements.is_empty() {
                return Err(step_error(index, id, "no matching element"));
            }
            return Ok((
                "assert_element".into(),
                Some(json!({ "filter": query.filter, "matches": elements.len() })),
            ));
        }

        if let Some(query) = &step.list_elements {
            let device_id = selected_device_id(index, id, context)?;
            if self.dry_run {
                return Ok((
                    "list_elements".into(),
                    Some(json!({ "filter": query.filter, "max_results": query.max_results })),
                ));
            }
            let elements = self
                .robot(index, id)?
                .list_elements_filtered(device_id, Some(&query.filter), query.max_results)
                .await
                .map_err(|error| step_error(index, id, error))?;
            return Ok(("list_elements".into(), Some(json!(elements))));
        }

        Err(step_error(index, id, "step has no recognized action"))
    }

    fn robot(&self, index: usize, id: &str) -> Result<&Arc<dyn Robot>, FlowError> {
        self.robot.as_ref().ok_or_else(|| {
            step_error(
                index,
                id,
                "this step requires a robot; use FlowRunner::with_robot for execution",
            )
        })
    }
}

fn select_device(
    selection: &SelectDeviceStep,
    devices: &[RobotDevice],
) -> Result<RobotDevice, String> {
    let matches: Vec<_> = devices
        .iter()
        .filter(|device| selection.id.as_ref().is_none_or(|id| &device.id == id))
        .filter(|device| {
            selection
                .name
                .as_ref()
                .is_none_or(|name| &device.name == name)
        })
        .filter(|device| {
            selection
                .platform
                .as_ref()
                .is_none_or(|platform| &device.platform == platform)
        })
        .filter(|device| {
            selection
                .state
                .as_ref()
                .is_none_or(|state| &device.state == state)
        })
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => Err("no device matched select_device criteria".into()),
        [device] => Ok(device.clone()),
        _ => Err(format!(
            "select_device matched {} devices; add an id or name",
            matches.len()
        )),
    }
}

fn selected_device_id<'a>(
    index: usize,
    id: &str,
    context: &'a FlowContext,
) -> Result<&'a str, FlowError> {
    context
        .selected_device
        .as_ref()
        .map(|device| device.id.as_str())
        .ok_or_else(|| step_error(index, id, "select_device must run before this step"))
}

fn center(element: &UiElement) -> Point {
    Point {
        x: f64::from(element.bounds.left + element.bounds.right) / 2.0,
        y: f64::from(element.bounds.top + element.bounds.bottom) / 2.0,
    }
}

fn step_id(index: usize, step: &FlowStep) -> String {
    step.id
        .clone()
        .unwrap_or_else(|| format!("step-{}", index + 1))
}

fn step_error(index: usize, id: &str, error: impl ToString) -> FlowError {
    FlowError::Step {
        index,
        id: id.into(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mobile_robot_core::RobotError;

    #[derive(Default)]
    struct FakeRobot;

    #[async_trait]
    impl Robot for FakeRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(vec![RobotDevice {
                id: "fake-macos".into(),
                name: "Fake Mac".into(),
                platform: Platform::Macos,
                state: DeviceState::Available,
            }])
        }

        async fn take_screenshot(&self, _device_id: &str) -> Result<Vec<u8>, RobotError> {
            Ok(vec![137, 80, 78, 71])
        }

        async fn tap(&self, _device_id: &str, _point: Point) -> Result<(), RobotError> {
            Ok(())
        }

        async fn launch_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn open_url(
            &self,
            _device_id: &str,
            _url: &str,
            _app_id: Option<&str>,
        ) -> Result<(), RobotError> {
            Ok(())
        }

        async fn type_text(&self, _device_id: &str, _text: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn swipe(
            &self,
            _device_id: &str,
            _start: Point,
            _end: Point,
            _duration_ms: Option<u64>,
        ) -> Result<(), RobotError> {
            Ok(())
        }

        async fn install_app(&self, _device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn uninstall_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
            Ok(())
        }

        async fn list_elements(&self, _device_id: &str) -> Result<Vec<UiElement>, RobotError> {
            Ok(vec![])
        }
    }

    #[test]
    fn parses_and_validates_a_yaml_flow() {
        let flow = FlowDocument::from_yaml(
            r#"
version: 1
name: smoke
steps:
  - id: select
    select_device:
      id: fake-macos
      platform: macos
  - id: launch
    launch_app:
      app_id: Safari
"#,
        )
        .unwrap();
        assert_eq!(flow.steps.len(), 2);
    }

    #[tokio::test]
    async fn dry_run_executes_steps_without_a_robot() {
        let flow = FlowDocument::from_yaml(
            r#"
version: 1
name: dry-run
steps:
  - select_device:
      id: fake-macos
  - wait:
      ms: 1
  - launch_app:
      app_id: Safari
"#,
        )
        .unwrap();
        let report = FlowRunner::dry_run().run(&flow).await.unwrap();
        assert!(report.dry_run);
        assert_eq!(report.steps.len(), 3);
        assert_eq!(report.selected_device.unwrap().id, "fake-macos");
    }

    #[tokio::test]
    async fn select_device_stays_inside_flow_runner() {
        let flow = FlowDocument::from_yaml(
            r#"
version: 1
name: execute
steps:
  - select_device:
      platform: macos
  - launch_app:
      app_id: Safari
"#,
        )
        .unwrap();
        let report = FlowRunner::with_robot(Arc::new(FakeRobot))
            .run(&flow)
            .await
            .unwrap();
        assert_eq!(report.selected_device.unwrap().id, "fake-macos");
    }
}

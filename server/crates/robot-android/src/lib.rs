//! Android implementation of the platform-neutral [`Robot`] contract.
//!
//! Process execution is isolated behind [`CommandRunner`], letting the ADB
//! parser and command construction be tested without an emulator or `adb`.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use robot_core::{
    Bounds, DeviceState, ElementType, ImeAction, Platform, Point, Robot, RobotDevice, RobotError,
    UiElement, prepare_elements,
};
use serde_json::{Value, json};
use tokio::{process::Command, sync::Mutex};
use url::Url;

mod emulator;
mod video;

static TREE_DUMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct ActiveRecording {
    device_path: String,
    artifact_path: PathBuf,
    pid: u32,
}

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

    async fn spawn_background(
        &self,
        _executable: &str,
        _args: &[String],
        _log_path: &std::path::Path,
    ) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("background process launch".into()))
    }
}

/// Real process runner used outside tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn spawn_background(
        &self,
        executable: &str,
        args: &[String],
        log_path: &std::path::Path,
    ) -> Result<(), RobotError> {
        let log = std::fs::File::create(log_path).map_err(|error| {
            RobotError::CommandFailed(format!("could not create emulator log: {error}"))
        })?;
        let stderr = log.try_clone().map_err(|error| {
            RobotError::CommandFailed(format!("could not clone emulator log: {error}"))
        })?;
        Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(stderr)
            .kill_on_drop(false)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start {executable}: {error}"))
            })?;
        Ok(())
    }

    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        let output = Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
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
    emulator_path: String,
    boot_lock: Arc<Mutex<()>>,
    artifact_dir: PathBuf,
    recordings: Arc<Mutex<HashMap<String, ActiveRecording>>>,
    tree_dump_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    runner: R,
    video_controls: Arc<video::Controls>,
}

impl AndroidRobot<TokioCommandRunner> {
    pub fn new(adb_path: impl Into<String>, artifact_dir: impl Into<PathBuf>) -> Self {
        Self::with_runner_and_artifact_dir(adb_path, artifact_dir, TokioCommandRunner)
    }
}

impl<R> AndroidRobot<R> {
    pub fn with_runner(adb_path: impl Into<String>, runner: R) -> Self {
        Self::with_runner_and_artifact_dir(adb_path, "artifacts", runner)
    }

    pub fn with_runner_and_artifact_dir(
        adb_path: impl Into<String>,
        artifact_dir: impl Into<PathBuf>,
        runner: R,
    ) -> Self {
        let adb_path = adb_path.into();
        Self {
            emulator_path: emulator::emulator_path(&adb_path),
            adb_path,
            boot_lock: Arc::default(),
            artifact_dir: artifact_dir.into(),
            recordings: Arc::default(),
            tree_dump_locks: Arc::default(),
            runner,
            video_controls: Arc::default(),
        }
    }

    pub fn adb_path(&self) -> &str {
        &self.adb_path
    }

    pub fn with_emulator_path(mut self, path: impl Into<String>) -> Self {
        self.emulator_path = path.into();
        self
    }

    fn next_recording(&self, device_id: &str) -> ActiveRecording {
        let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let safe_device_id = device_id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        ActiveRecording {
            device_path: format!("/sdcard/mobile-mcp-{sequence}.mp4"),
            artifact_path: self.artifact_dir.join(format!(
                "mobile-mcp-android-recording-{safe_device_id}-{sequence}.mp4"
            )),
            pid: 0,
        }
    }
}

#[async_trait]
impl<R: CommandRunner> Robot for AndroidRobot<R> {
    async fn start_video_stream(
        &self,
        device_id: &str,
    ) -> Result<robot_core::VideoStream, RobotError> {
        ensure_device_id(device_id)?;
        video::start(self.adb_path(), device_id, &self.video_controls).await
    }
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.discover_devices().await
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        self.boot_emulator(device_id).await
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "pm".into(),
                "list".into(),
                "packages".into(),
                "-f".into(),
            ])
            .await?;
        let text = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!(
                "adb package listing returned invalid UTF-8: {error}"
            ))
        })?;
        Ok(json!({
            "platform": "android",
            "deviceId": device_id,
            "apps": parse_adb_installed_apps(&text),
        }))
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        ensure_device_id(device_id)?;
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

        let mut recording = self.next_recording(device_id);
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "rm".into(),
            "-f".into(),
            recording.device_path.clone(),
        ])
        .await?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                format!(
                    "setsid screenrecord --time-limit 1800 {} >/dev/null 2>&1 & echo $!",
                    recording.device_path
                ),
            ])
            .await?;
        let pid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                RobotError::CommandFailed(
                    "adb did not return a screenrecord process id; device may not support screenrecord"
                        .into(),
                )
            })?;
        recording.pid = pid;
        let artifact_path = recording.artifact_path.to_string_lossy().into_owned();
        recordings.insert(device_id.into(), recording);
        Ok(artifact_path)
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        ensure_device_id(device_id)?;
        let Some(recording) = self.recordings.lock().await.remove(device_id) else {
            return Err(RobotError::InvalidInput(format!(
                "no screen recording is running for device {device_id}"
            )));
        };
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            format!(
                "kill -INT {}; i=0; while kill -0 {} 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done",
                recording.pid, recording.pid
            ),
        ])
        .await?;
        let device_path = recording.device_path.clone();
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "pull".into(),
            device_path.clone(),
            recording.artifact_path.to_string_lossy().into_owned(),
        ])
        .await?;
        // The transferred MP4 is now durable on the host. Best-effort cleanup
        // avoids accumulating recordings on a physical device.
        let _ = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "rm".into(),
                "-f".into(),
                device_path,
            ])
            .await;
        Ok(recording.artifact_path.to_string_lossy().into_owned())
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

    async fn terminate_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "am".into(),
            "force-stop".into(),
            "--user".into(),
            "current".into(),
            app_id.into(),
        ])
        .await?;
        Ok(())
    }

    async fn terminate_all_apps(&self, device_id: &str) -> Result<Vec<String>, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "pm".into(),
                "list".into(),
                "packages".into(),
                "-3".into(),
                "--user".into(),
                "current".into(),
            ])
            .await?;
        let mut packages = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            let package = line.trim().strip_prefix("package:").ok_or_else(|| {
                RobotError::CommandFailed("Unexpected Android package list; no apps stopped".into())
            })?;
            ensure_app_id(package)?;
            packages.push(package.to_owned());
        }
        packages.sort();
        packages.dedup();
        for (count, package) in packages.iter().enumerate() {
            self.terminate_app(device_id, package)
                .await
                .map_err(|error| {
                    RobotError::CommandFailed(format!(
                        "Stopped {count} apps; could not stop {package}: {error}"
                    ))
                })?;
        }
        Ok(packages)
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

    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        let output = self
            .run_adb(&[
                "-s".into(),
                id.into(),
                "shell".into(),
                "dumpsys".into(),
                "activity".into(),
                "activities".into(),
            ])
            .await?;
        let text = String::from_utf8_lossy(&output.stdout);
        let app = text
            .lines()
            .find(|line| line.contains("mResumedActivity") || line.contains("topResumedActivity"))
            .and_then(|line| line.split_whitespace().find(|part| part.contains('/')))
            .and_then(|part| part.split('/').next());
        Ok(json!({"appId":app}))
    }
    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let elements = self.list_elements(device_id).await?;
        if !elements.iter().any(|element| {
            element.focused == Some(true)
                && element.enabled
                && element.element_type == Some(ElementType::TextField)
        }) {
            return Err(RobotError::InvalidInput(
                "Focus an editable Android field before clearing text".into(),
            ));
        }
        let temporary = if self.video_controls.available(device_id).await {
            None
        } else {
            Some(video::start(self.adb_path(), device_id, &self.video_controls).await?)
        };
        let result = self.video_controls.clear_text(device_id).await;
        drop(temporary);
        result
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

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "dumpsys".into(),
                "input_method".into(),
            ])
            .await?;
        let dumpsys = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!(
                "dumpsys input_method returned invalid UTF-8: {error}"
            ))
        })?;
        let observed = parse_focused_ime_action(&dumpsys)?;
        if observed != action {
            return Err(RobotError::CommandFailed(format!(
                "focused editor IME action is {}, not {}; KEYCODE_ENTER was not sent",
                observed.as_str(),
                action.as_str()
            )));
        }
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "keyevent".into(),
            "66".into(),
        ])
        .await?;
        Ok(())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let start_x = android_coordinate(start.x, "startX")?;
        let start_y = android_coordinate(start.y, "startY")?;
        let end_x = android_coordinate(end.x, "endX")?;
        let end_y = android_coordinate(end.y, "endY")?;
        let duration = duration_ms.unwrap_or(300);
        if !(1..=60_000).contains(&duration) {
            return Err(RobotError::InvalidInput(
                "durationMs must be between 1 and 60000".into(),
            ));
        }
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "swipe".into(),
            start_x.to_string(),
            start_y.to_string(),
            end_x.to_string(),
            end_y.to_string(),
            duration.to_string(),
        ])
        .await?;
        Ok(())
    }

    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_apk_path(apk_path)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "install".into(),
            "-r".into(),
            apk_path.into(),
        ])
        .await?;
        Ok(())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "uninstall".into(),
            app_id.into(),
        ])
        .await?;
        Ok(())
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        ensure_device_id(device_id)?;
        // Android allows one UiAutomation service per device. QC Live and flow
        // replay share this driver (including clones), so serialize the entire
        // dump/read/cleanup here rather than only gating the live HTTP route.
        let gate = {
            let mut gates = self.tree_dump_locks.lock().await;
            gates.entry(device_id.to_owned()).or_default().clone()
        };
        let _guard = gate.lock().await;
        let sequence = TREE_DUMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let remote_path = format!("/sdcard/mobile-mcp-{sequence}.xml");
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "uiautomator".into(),
            "dump".into(),
            "--compressed".into(),
            remote_path.clone(),
        ])
        .await?;

        let xml_output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "exec-out".into(),
                "cat".into(),
                remote_path.clone(),
            ])
            .await;
        // Always attempt cleanup once the dump has been created; a cleanup
        // failure must not hide an otherwise usable accessibility snapshot.
        let _ = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "rm".into(),
                "-f".into(),
                remote_path,
            ])
            .await;

        let xml = String::from_utf8(xml_output?.stdout).map_err(|error| {
            RobotError::CommandFailed(format!("uiautomator dump returned invalid UTF-8: {error}"))
        })?;
        parse_uiautomator_xml(&xml)
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
    if device_id.starts_with("avd:") {
        return Err(RobotError::InvalidInput(
            "start the AVD first, refresh devices, then use its emulator-* ADB id".into(),
        ));
    }
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

fn ensure_apk_path(apk_path: &str) -> Result<(), RobotError> {
    if apk_path.trim().is_empty() || !apk_path.ends_with(".apk") {
        return Err(RobotError::InvalidInput(
            "apk path must name a non-empty .apk file".into(),
        ));
    }
    Ok(())
}

/// Parses an Android `uiautomator dump` XML document into portable elements.
pub fn parse_uiautomator_xml(xml: &str) -> Result<Vec<UiElement>, RobotError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut elements = vec![];

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(node)) | Ok(Event::Start(node)) if node.name().as_ref() == b"node" => {
                elements.push(parse_node(&node, reader.decoder())?);
            }
            Ok(_) => {}
            Err(error) => {
                return Err(RobotError::CommandFailed(format!(
                    "could not parse uiautomator XML: {error}"
                )));
            }
        }
    }
    Ok(elements)
}

fn parse_node(
    node: &BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<UiElement, RobotError> {
    let mut attributes = HashMap::new();
    for attribute in node.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            RobotError::CommandFailed(format!("could not parse uiautomator attribute: {error}"))
        })?;
        let key = std::str::from_utf8(attribute.key.as_ref()).map_err(|error| {
            RobotError::CommandFailed(format!("invalid attribute name: {error}"))
        })?;
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|error| {
                RobotError::CommandFailed(format!("invalid attribute value: {error}"))
            })?;
        attributes.insert(key.to_owned(), value.into_owned());
    }

    let bounds = attributes
        .get("bounds")
        .ok_or_else(|| RobotError::CommandFailed("uiautomator node has no bounds".into()))
        .and_then(|value| parse_bounds(value))?;
    let text = optional_attribute(&mut attributes, "text");
    let mut element = UiElement {
        class_name: attributes.remove("class").unwrap_or_default(),
        resource_id: optional_attribute(&mut attributes, "resource-id"),
        text,
        content_description: optional_attribute(&mut attributes, "content-desc"),
        bounds,
        clickable: attributes
            .get("clickable")
            .is_some_and(|value| value == "true"),
        enabled: attributes
            .get("enabled")
            .is_none_or(|value| value == "true"),
        focused: optional_bool(&attributes, "focused"),
        checked: optional_bool(&attributes, "checked"),
        selected: optional_bool(&attributes, "selected"),
        value: None,
        element_type: None,
        web_context: None,
    };
    prepare_elements(std::slice::from_mut(&mut element));
    if element.element_type == Some(ElementType::TextField) {
        element.value = element.text.clone();
    }
    Ok(element)
}

fn optional_bool(attributes: &HashMap<String, String>, key: &str) -> Option<bool> {
    match attributes.get(key).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// Reads the focused editor's `imeOptions` action. A null served view or an
/// action other than next/done/go/search/send is an error so callers do not
/// send KEYCODE_ENTER for a different IME action.
pub fn parse_focused_ime_action(dumpsys: &str) -> Result<ImeAction, RobotError> {
    if last_assignment(dumpsys, "mServedView")
        .is_some_and(|value| value == "null" || value.is_empty())
    {
        return Err(RobotError::CommandFailed(
            "no focused editor is available for an IME action".into(),
        ));
    }
    let raw = last_ime_options(dumpsys).ok_or_else(|| {
        RobotError::CommandFailed(
            "focused editor did not report imeOptions; KEYCODE_ENTER was not sent".into(),
        )
    })?;
    let action_id = raw & 0xff;
    ImeAction::from_android_action_id(action_id).ok_or_else(|| {
        RobotError::CommandFailed(format!(
            "focused editor imeOptions action id {action_id} is not next, done, go, search, or send; KEYCODE_ENTER was not sent"
        ))
    })
}

fn last_assignment<'a>(dumpsys: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=");
    let mut found = None;
    let mut rest = dumpsys;
    while let Some(index) = rest.find(&pattern) {
        let after = &rest[index + pattern.len()..];
        let token = after.split_whitespace().next().unwrap_or("");
        found = Some(token.trim_matches(|character| character == ',' || character == '}'));
        rest = after.get(token.len()..).unwrap_or("");
    }
    found
}

fn last_ime_options(dumpsys: &str) -> Option<u32> {
    let mut found = None;
    let mut rest = dumpsys;
    while let Some(index) = rest.find("imeOptions=") {
        let after = &rest[index + "imeOptions=".len()..];
        let token = after
            .split(|character: char| {
                character.is_whitespace() || matches!(character, ',' | '}' | '"' | ';')
            })
            .next()
            .unwrap_or("");
        if let Some(value) = parse_ime_token(token) {
            found = Some(value);
        }
        rest = after.get(token.len()..).unwrap_or("");
    }
    found
}

fn parse_ime_token(token: &str) -> Option<u32> {
    let token = token.trim();
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        token.parse().ok()
    }
}

fn optional_attribute(attributes: &mut HashMap<String, String>, key: &str) -> Option<String> {
    attributes.remove(key).filter(|value| !value.is_empty())
}

fn parse_bounds(value: &str) -> Result<Bounds, RobotError> {
    let values: Result<Vec<i32>, _> = value
        .split(['[', ']', ','])
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect();
    let values = values.map_err(|error| {
        RobotError::CommandFailed(format!("invalid uiautomator bounds {value:?}: {error}"))
    })?;
    match values.as_slice() {
        [left, top, right, bottom] => Ok(Bounds {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
        }),
        _ => Err(RobotError::CommandFailed(format!(
            "invalid uiautomator bounds: {value:?}"
        ))),
    }
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

/// Parses `adb shell pm list packages -f` output into portable app records.
pub fn parse_adb_installed_apps(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| {
            let package = line.trim().strip_prefix("package:")?;
            let (source_path, package_name) = package.rsplit_once('=')?;
            Some(json!({
                "packageName": package_name,
                "sourcePath": source_path,
            }))
        })
        .collect()
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
    pub(super) struct FakeRunner {
        outputs: Mutex<VecDeque<CommandOutput>>,
        pub(super) calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeRunner {
        pub(super) fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().collect()),
                calls: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn spawn_background(
            &self,
            executable: &str,
            args: &[String],
            _: &std::path::Path,
        ) -> Result<(), RobotError> {
            self.calls
                .lock()
                .unwrap()
                .push((executable.into(), args.to_vec()));
            Ok(())
        }
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

    pub(super) fn successful(stdout: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            success: true,
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: vec![],
        }
    }

    // Android permits only one UiAutomation registration per device. Model the
    // real exit 137 when QC inspection and flow replay dump the same tree at once.
    #[derive(Debug, Default, Clone)]
    struct ConcurrentTreeRunner {
        active: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        barrier: Option<Arc<tokio::sync::Barrier>>,
    }

    #[async_trait]
    impl CommandRunner for ConcurrentTreeRunner {
        async fn run(&self, _: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
            if args.get(3).is_some_and(|arg| arg == "uiautomator") {
                let device = args[1].clone();
                if !self.active.lock().unwrap().insert(device.clone()) {
                    return Ok(CommandOutput {
                        success: false,
                        exit_code: Some(137),
                        stdout: vec![],
                        stderr: vec![],
                    });
                }
                if let Some(barrier) = &self.barrier {
                    barrier.wait().await;
                } else {
                    tokio::task::yield_now().await;
                }
                self.active.lock().unwrap().remove(&device);
            }
            if args.get(2).is_some_and(|arg| arg == "exec-out") {
                return Ok(successful(
                    r#"<hierarchy><node text="Import video" class="android.widget.Button" clickable="true" enabled="true" bounds="[0,0][100,40]" /></hierarchy>"#,
                ));
            }
            Ok(successful(""))
        }
    }

    #[tokio::test]
    async fn concurrent_tree_reads_on_one_device_do_not_kill_uiautomator() {
        let robot = AndroidRobot::with_runner("adb-test", ConcurrentTreeRunner::default());
        let replay = robot.clone();
        let (live, flow) =
            tokio::join!(robot.list_elements("phone"), replay.list_elements("phone"),);
        assert_eq!(live.unwrap().len(), 1);
        assert_eq!(flow.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tree_reads_on_different_devices_can_run_concurrently() {
        let runner = ConcurrentTreeRunner {
            barrier: Some(Arc::new(tokio::sync::Barrier::new(2))),
            ..Default::default()
        };
        let robot = AndroidRobot::with_runner("adb-test", runner);
        let reads =
            async { tokio::join!(robot.list_elements("first"), robot.list_elements("second")) };
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(1), reads)
            .await
            .expect("different devices must not share a dump lock");
        assert_eq!(first.unwrap().len(), 1);
        assert_eq!(second.unwrap().len(), 1);
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

    #[test]
    fn parses_installed_packages_with_apk_paths() {
        let apps = parse_adb_installed_apps(
            "package:/data/app/~~abc==/com.example.one-xyz==/base.apk=com.example.one\n\
             package:/system/priv-app/Settings/Settings.apk=com.android.settings\n",
        );

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0]["packageName"], "com.example.one");
        assert_eq!(
            apps[1]["sourcePath"],
            "/system/priv-app/Settings/Settings.apk"
        );
    }

    #[test]
    fn parses_uiautomator_nodes_without_losing_accessibility_metadata() {
        let elements = parse_uiautomator_xml(
            r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
            <hierarchy rotation="0">
              <node index="0" text="Search" resource-id="com.android.chrome:id/search_box" class="android.widget.EditText" package="com.android.chrome" content-desc="Search query" clickable="true" enabled="true" bounds="[10,20][300,80]" />
            </hierarchy>"#,
        )
        .unwrap();

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].text.as_deref(), Some("Search"));
        assert_eq!(
            elements[0].resource_id.as_deref(),
            Some("com.android.chrome:id/search_box")
        );
        assert_eq!(
            elements[0].bounds,
            Bounds {
                left: 10,
                top: 20,
                right: 300,
                bottom: 80,
            }
        );
        assert!(elements[0].clickable);
        assert_eq!(elements[0].focused, None);
        assert_eq!(elements[0].checked, None);
        assert_eq!(elements[0].element_type, Some(ElementType::TextField));
        assert_eq!(elements[0].value.as_deref(), Some("Search"));
    }

    #[test]
    fn type_filter_keeps_a_password_field_that_follows_other_controls() {
        let elements = parse_uiautomator_xml(
            r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
            <hierarchy>
              <node text="Save" class="android.widget.Button" clickable="true" enabled="true" bounds="[0,0][10,10]" />
              <node text="" class="android.widget.EditText" password="true" resource-id="password" focused="true" bounds="[0,20][10,40]" />
              <node text="user@example.com" class="android.widget.EditText" checked="false" selected="false" bounds="[0,50][10,70]" />
            </hierarchy>"#,
        )
        .unwrap();
        let found = robot_core::apply_element_query(
            elements,
            &robot_core::ElementQuery {
                filter: None,
                element_type: Some(ElementType::TextField),
                max_results: Some(1),
                web_context: None,
                include_frames: false,
            },
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].resource_id.as_deref(), Some("password"));
        assert_eq!(found[0].focused, Some(true));
        assert_eq!(found[0].value, None);

        let email = robot_core::apply_element_query(
            parse_uiautomator_xml(
                r#"<hierarchy>
                  <node text="Save" class="android.widget.Button" bounds="[0,0][10,10]" />
                  <node text="" class="android.widget.EditText" password="true" resource-id="password" bounds="[0,20][10,40]" />
                  <node text="user@example.com" class="android.widget.EditText" selected="false" bounds="[0,50][10,70]" />
                </hierarchy>"#,
            )
            .unwrap(),
            &robot_core::ElementQuery {
                filter: Some("user@example.com".into()),
                element_type: Some(ElementType::TextField),
                max_results: Some(2),
                        web_context: None,
            include_frames: false,
},
        );
        assert_eq!(email.len(), 1);
        assert_eq!(email[0].text.as_deref(), Some("user@example.com"));
        assert_eq!(email[0].selected, Some(false));
        assert_eq!(email[0].checked, None);
    }

    #[test]
    fn reads_the_focused_editor_action_and_rejects_a_mismatch() {
        let next = "mServedView=android.view.View\n    imeOptions=0x5\n";
        assert_eq!(parse_focused_ime_action(next).unwrap(), ImeAction::Next);
        let done = "mCurrentTextBoxAttribute: EditorInfo{imeOptions=0x10000006}";
        assert_eq!(parse_focused_ime_action(done).unwrap(), ImeAction::Done);
        assert!(
            parse_focused_ime_action("mServedView=null\nimeOptions=6\n")
                .unwrap_err()
                .to_string()
                .contains("no focused editor")
        );
        assert!(
            parse_focused_ime_action("mServedView=android.view.View\nimeOptions=1\n")
                .unwrap_err()
                .to_string()
                .contains("KEYCODE_ENTER was not sent")
        );
    }

    #[tokio::test]
    async fn clear_rejects_non_editable_focus_before_sending_keys() {
        let runner = FakeRunner::new([
            successful(""),
            successful(
                r#"<hierarchy><node text="Button" class="android.widget.Button" focused="true" enabled="true" bounds="[0,0][100,40]" /></hierarchy>"#,
            ),
            successful(""),
        ]);
        let robot = AndroidRobot::with_runner("adb-test", runner);
        let error = robot.clear_text("phone").await.unwrap_err();
        assert!(error.to_string().contains("Focus an editable"));
        assert_eq!(robot.runner.calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn sends_enter_only_when_the_focused_ime_action_matches() {
        let runner = FakeRunner::new([
            successful("mServedView=android.widget.EditText\nimeOptions=0x10000005\n"),
            successful(vec![]),
        ]);
        let robot = AndroidRobot::with_runner("adb-test", runner);
        robot
            .press_ime_action("emulator-5554", ImeAction::Next)
            .await
            .unwrap();
        {
            let calls = robot.runner.calls.lock().unwrap();
            assert_eq!(calls[0].1[2..], ["shell", "dumpsys", "input_method"]);
            assert_eq!(calls[1].1[2..], ["shell", "input", "keyevent", "66"]);
        }

        let mismatch = FakeRunner::new([successful(
            "mServedView=android.widget.EditText\nimeOptions=5\n",
        )]);
        let robot = AndroidRobot::with_runner("adb-test", mismatch);
        let error = robot
            .press_ime_action("emulator-5554", ImeAction::Done)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("not done"));
        assert!(error.contains("KEYCODE_ENTER was not sent"));
        assert_eq!(robot.runner.calls.lock().unwrap().len(), 1);
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
    async fn terminates_apps_for_current_user_and_rejects_invalid_packages() {
        let robot = AndroidRobot::with_runner(
            "adb-test",
            FakeRunner::new([
                successful(""),
                successful(
                    "package:com.example.two\npackage:com.example.one\npackage:com.example.one\n",
                ),
                successful(""),
                successful(""),
            ]),
        );
        robot
            .terminate_app("emulator-5554", "com.example.focused")
            .await
            .unwrap();
        let apps = robot.terminate_all_apps("emulator-5554").await.unwrap();
        assert_eq!(apps, ["com.example.one", "com.example.two"]);
        let calls = robot.runner.calls.lock().unwrap().clone();
        assert_eq!(
            calls[0].1,
            [
                "-s",
                "emulator-5554",
                "shell",
                "am",
                "force-stop",
                "--user",
                "current",
                "com.example.focused"
            ]
        );
        assert_eq!(
            calls[1].1,
            [
                "-s",
                "emulator-5554",
                "shell",
                "pm",
                "list",
                "packages",
                "-3",
                "--user",
                "current"
            ]
        );
        assert_eq!(calls[2].1.last().unwrap(), "com.example.one");
        drop(calls);
        assert!(
            robot
                .terminate_app("emulator-5554", "com.app;reboot")
                .await
                .is_err()
        );
        assert_eq!(robot.runner.calls.lock().unwrap().len(), 4);
        let malformed = AndroidRobot::with_runner(
            "adb-test",
            FakeRunner::new([successful("package:com.good.app\npackage:bad;reboot\n")]),
        );
        assert!(malformed.terminate_all_apps("emulator-5554").await.is_err());
        assert_eq!(malformed.runner.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reports_partial_app_termination_failure() {
        let robot = AndroidRobot::with_runner(
            "adb-test",
            FakeRunner::new([
                successful("package:com.example.one\npackage:com.example.two\n"),
                successful(""),
                CommandOutput {
                    success: false,
                    exit_code: Some(1),
                    stdout: vec![],
                    stderr: b"denied".to_vec(),
                },
            ]),
        );
        let error = robot
            .terminate_all_apps("emulator-5554")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Stopped 1 apps"), "{error}");
        assert!(error.contains("com.example.two"), "{error}");
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

    #[tokio::test]
    async fn records_with_a_device_pid_and_pulls_the_final_mp4() {
        let runner = FakeRunner::new([
            successful(vec![]),
            successful("4242\n"),
            successful(vec![]),
            successful(vec![]),
            successful(vec![]),
        ]);
        let artifact_dir = std::env::temp_dir().join("robot-android-recording-test");
        let robot = AndroidRobot::with_runner_and_artifact_dir("adb-test", &artifact_dir, runner);

        let started = robot.start_screen_recording("emulator-5554").await.unwrap();
        let stopped = robot.stop_screen_recording("emulator-5554").await.unwrap();

        assert_eq!(started, stopped);
        assert!(stopped.ends_with(".mp4"));
        let calls = robot.runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0].1[2..5], ["shell", "rm", "-f"]);
        assert_eq!(calls[1].1[2], "shell");
        assert!(calls[1].1[3].starts_with("setsid screenrecord --time-limit 1800 /sdcard/"));
        assert_eq!(calls[2].1[2], "shell");
        assert!(calls[2].1[3].starts_with("kill -INT 4242"));
        assert_eq!(calls[3].1[2], "pull");
        assert_eq!(calls[3].1[4], stopped);
        assert_eq!(calls[4].1[2..5], ["shell", "rm", "-f"]);
    }
}

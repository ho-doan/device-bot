//! Native Windows desktop driver backed by PowerShell and WinForms.
//!
//! The process must run in an interactive Windows session. Desktop input can
//! be blocked by UAC's secure desktop or by a policy that disallows UI access.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use accessibility::{build_uia_script, msaa};
#[cfg(test)]
use robot_core::Robot;
use robot_core::{Bounds, ElementQuery, Point, RobotError, UiElement, apply_element_query};
use serde_json::{Value, json};
use tokio::{process::Command, sync::Mutex};

mod accessibility;
mod adapter;
mod flutter_bridge;
mod live;
mod recording;

const DEVICE_ID: &str = "windows-host";
const MAX_UI_NODES: usize = 5_000;
const MAX_UI_DEPTH: usize = 128;
// This budget includes starting PowerShell, loading UIA assemblies and compiling
// its helper, not just traversing the app. Cold Windows/RDP starts can use the
// old 2.5s budget before the first element is visited.
const UIA_QUERY_TIMEOUT: Duration = Duration::from_secs(10);
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct WindowsRobot {
    artifact_dir: PathBuf,
    powershell_path: String,
    active_process_id: Arc<Mutex<Option<u32>>>,
    active_window_id: Arc<Mutex<Option<i64>>>,
    recording: Arc<Mutex<recording::DesktopRecorder>>,
    capture_lock: Arc<Mutex<()>>,
    live_geometry: Arc<Mutex<std::collections::HashMap<String, live::Geometry>>>,
}

impl WindowsRobot {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifact_dir: artifact_dir.into(),
            powershell_path: std::env::var("DEVICE_BOT_POWERSHELL_PATH")
                .unwrap_or_else(|_| "powershell.exe".into()),
            active_process_id: Arc::new(Mutex::new(None)),
            active_window_id: Arc::default(),
            recording: Arc::new(Mutex::new(recording::DesktopRecorder::default())),
            capture_lock: Arc::default(),
            live_geometry: Arc::default(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()))
    }

    fn require_device(&self, device_id: &str) -> Result<(), RobotError> {
        if device_id != DEVICE_ID && live::Target::parse(device_id)?.is_none() {
            return Err(RobotError::DeviceNotFound(device_id.into()));
        }
        if !cfg!(target_os = "windows") {
            return Err(RobotError::Unsupported(
                "the Windows robot is available only when the MCP server runs on Windows".into(),
            ));
        }
        Ok(())
    }

    async fn powershell(&self, script: String) -> Result<Vec<u8>, RobotError> {
        // Windows PowerShell otherwise encodes stdout in the OEM code page.
        // A window title outside that page then makes the whole JSON unreadable.
        let script = utf8_powershell(script);
        let mut command = Command::new(&self.powershell_path);
        command
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
            .stdin(Stdio::null())
            .kill_on_drop(true);
        // A console spawned between activation and SendInput can steal focus
        // from the target. Keep every driver helper invisible on Windows.
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let output = command.output().await.map_err(|error| {
            RobotError::CommandFailed(format!("could not start PowerShell: {error}"))
        })?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RobotError::CommandFailed(format!(
                "PowerShell exited with {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".into(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    async fn uia_elements(&self, max_results: Option<usize>) -> Result<Vec<UiElement>, RobotError> {
        if max_results == Some(0) {
            return Ok(vec![]);
        }

        // UIA can query the selected process while QC remains foreground.
        let process_id = *self.active_process_id.lock().await;
        let window_id = *self.active_window_id.lock().await;
        if msaa::use_msaa(
            std::env::var("DEVICE_BOT_WINDOWS_ACCESSIBILITY")
                .ok()
                .as_deref(),
        )? {
            return self.msaa_elements(process_id, window_id, max_results).await;
        }
        if let (Some(pid), Some(hwnd)) = (process_id, window_id) {
            return self
                .live_elements(&format!("windows:{pid}:{hwnd}"), max_results)
                .await;
        }
        let script = build_uia_script(process_id, max_results);
        let output = tokio::time::timeout(UIA_QUERY_TIMEOUT, self.powershell(script))
            .await
            .map_err(|_| {
                RobotError::CommandFailed(format!(
                    "Windows UI Automation helper exceeded its {} second startup/traversal deadline",
                    UIA_QUERY_TIMEOUT.as_secs()
                ))
            })??;
        parse_uia_elements(&output)
    }

    async fn query_native_elements(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        // The helpers cap the native snapshot. Only an unfiltered request can
        // stop at the caller's result limit before portable matching runs.
        let max = query.max_results.filter(|&max| {
            max == 0
                || (query.element_type.is_none()
                    && query.filter.as_deref().is_none_or(str::is_empty))
        });
        let elements = if live::Target::parse(device_id)?.is_some() {
            self.live_elements(device_id, max).await?
        } else {
            self.uia_elements(max).await?
        };
        Ok(apply_element_query(elements, &query))
    }

    async fn foreground_process_id(&self) -> Result<u32, RobotError> {
        let output = self.powershell(r#"
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotForeground {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
$targetPid = [uint32]0
[void][DeviceBotForeground]::GetWindowThreadProcessId([DeviceBotForeground]::GetForegroundWindow(), [ref]$targetPid)
[Console]::WriteLine($targetPid)
"#.into()).await?;
        parse_process_id(&output)
    }

    async fn target_process_id(&self) -> Result<u32, RobotError> {
        let active = *self.active_process_id.lock().await;
        match active {
            Some(id) => Ok(id),
            None => self.foreground_process_id().await,
        }
    }

    async fn activate_target(&self) -> Result<(), RobotError> {
        let Some(id) = *self.active_process_id.lock().await else {
            return Ok(());
        };
        if let Some(hwnd) = *self.active_window_id.lock().await {
            let prefix = self
                .live_input_prefix(&format!("windows:{id}:{hwnd}"), &[])
                .await?;
            return self.powershell(prefix).await.map(|_| ());
        }
        self.powershell(screenshot::activate_script(id))
            .await
            .map(|_| ())
    }

    async fn listed_windows(
        &self,
        process_id: Option<u32>,
    ) -> Result<Vec<WindowProcess>, RobotError> {
        let script = live::window_list_script(process_id);
        let output = self.powershell(script).await?;
        parse_window_processes(&output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowProcess {
    process_id: u32,
    name: String,
    window_id: String,
    title: String,
}

fn parse_window_processes(output: &[u8]) -> Result<Vec<WindowProcess>, RobotError> {
    let value = parse_powershell_json(output).map_err(|error| {
        RobotError::CommandFailed(format!("PowerShell returned invalid window JSON: {error}"))
    })?;
    let rows = match value {
        Value::Null => vec![],
        Value::Array(values) => values,
        other => vec![other],
    };
    rows.into_iter()
        .map(|row| {
            let object = row.as_object().ok_or_else(|| {
                RobotError::CommandFailed("window list entries must be objects".into())
            })?;
            let process_id = object
                .get("processId")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0 && *value <= u64::from(u32::MAX))
                .ok_or_else(|| RobotError::CommandFailed("window processId is invalid".into()))?
                as u32;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .trim();
            Ok(WindowProcess {
                process_id,
                name: if name.is_empty() {
                    format!("pid {process_id}")
                } else {
                    name.into()
                },
                window_id: object
                    .get("windowId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                title: object
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

/// Windows PowerShell writes child stdout as UTF-8 without a BOM. A missing
/// console must not fail the script: redirected output still uses `$OutputEncoding`.
fn utf8_powershell(script: String) -> String {
    const PREFIX: &str = "$OutputEncoding = New-Object System.Text.UTF8Encoding $false\ntry { [Console]::OutputEncoding = $OutputEncoding } catch {}\n";
    format!("{PREFIX}{script}")
}

/// .NET window/UIA strings may contain unpaired UTF-16 code units, and a
/// console code page can emit bytes that are not UTF-8. Preserve valid pairs
/// and literal backslashes, replace isolated surrogates, and keep one bad
/// byte from rejecting the rest of the JSON.
fn parse_powershell_json(output: &[u8]) -> Result<Value, serde_json::Error> {
    let output = output.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(output);
    let unit = |at: usize| -> Option<u16> {
        if output.get(at..at + 2)? != b"\\u" {
            return None;
        }
        u16::from_str_radix(std::str::from_utf8(output.get(at + 2..at + 6)?).ok()?, 16).ok()
    };
    let mut normalized = Vec::with_capacity(output.len());
    let mut at = 0;
    let mut in_string = false;
    while at < output.len() {
        let byte = output[at];
        if byte == b'"' {
            in_string = !in_string;
        }
        if in_string && byte == b'\\' {
            if let Some(value) = unit(at) {
                if (0xd800..=0xdbff).contains(&value)
                    && unit(at + 6).is_some_and(|low| (0xdc00..=0xdfff).contains(&low))
                {
                    normalized.extend_from_slice(&output[at..at + 12]);
                    at += 12;
                    continue;
                }
                if (0xd800..=0xdfff).contains(&value) {
                    normalized.extend_from_slice(b"\\ufffd");
                    at += 6;
                    continue;
                }
            }
            // Consume escapes together so an escaped backslash cannot start
            // a Unicode escape and an escaped quote cannot end the string.
            let end = (at + 2).min(output.len());
            normalized.extend_from_slice(&output[at..end]);
            at = end;
            continue;
        }
        normalized.push(byte);
        at += 1;
    }
    serde_json::from_str(&String::from_utf8_lossy(&normalized))
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn mouse_helpers() -> &'static str {
    "Add-Type @'\nusing System;\nusing System.Runtime.InteropServices;\npublic static class MobileMcpMouse {\n  [DllImport(\"user32.dll\")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);\n}\n'@\nAdd-Type -AssemblyName System.Windows.Forms\nAdd-Type -AssemblyName System.Drawing"
}

fn parse_process_id(output: &[u8]) -> Result<u32, RobotError> {
    String::from_utf8_lossy(output)
        .lines()
        .rev()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches('\u{feff}')
                .parse::<u32>()
                .ok()
        })
        .find(|process_id| *process_id > 0)
        .ok_or_else(|| {
            RobotError::CommandFailed(
                "PowerShell did not return a usable Windows target process id".into(),
            )
        })
}

fn parse_uia_elements(output: &[u8]) -> Result<Vec<UiElement>, RobotError> {
    let output = output.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(output);
    let value = parse_powershell_json(output).map_err(|error| {
        RobotError::CommandFailed(format!(
            "PowerShell returned invalid UI Automation JSON: {error}"
        ))
    })?;
    let values = match value {
        Value::Null => vec![],
        Value::Array(values) => values,
        value => vec![value],
    };

    values
        .into_iter()
        .map(|value| {
            let object = value.as_object().ok_or_else(|| {
                RobotError::CommandFailed("Windows UI Automation returned a non-object node".into())
            })?;
            let required_string = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        RobotError::CommandFailed(format!(
                            "Windows UI Automation node is missing {key}"
                        ))
                    })
            };
            let coordinate = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| {
                        RobotError::CommandFailed(format!(
                            "Windows UI Automation node has an invalid {key} coordinate"
                        ))
                    })
            };
            let optional_string = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            };
            Ok(UiElement {
                class_name: required_string("className")?,
                resource_id: optional_string("resourceId"),
                text: optional_string("text"),
                content_description: optional_string("contentDescription"),
                bounds: Bounds {
                    left: coordinate("left")?,
                    top: coordinate("top")?,
                    right: coordinate("right")?,
                    bottom: coordinate("bottom")?,
                },
                clickable: object
                    .get("clickable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                enabled: object
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                focused: object.get("focused").and_then(Value::as_bool),
                checked: object.get("checked").and_then(Value::as_bool),
                selected: object.get("selected").and_then(Value::as_bool),
                value: object
                    .get("value")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                element_type: None,
                web_context: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;

mod screenshot;

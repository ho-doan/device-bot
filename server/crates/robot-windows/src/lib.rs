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

use async_trait::async_trait;
use robot_core::{
    Bounds, DeviceState, ElementQuery, ImeAction, Platform, Point, Robot, RobotDevice, RobotError,
    UiElement, apply_element_query,
};
use serde_json::{Value, json};
use tokio::{process::Command, sync::Mutex};

mod flutter_bridge;

const DEVICE_ID: &str = "windows-host";
const MAX_UI_NODES: usize = 5_000;
const MAX_UI_DEPTH: usize = 128;
const UIA_QUERY_TIMEOUT: Duration = Duration::from_millis(2_500);
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct WindowsRobot {
    artifact_dir: PathBuf,
    powershell_path: String,
    active_process_id: Arc<Mutex<Option<u32>>>,
}

impl WindowsRobot {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifact_dir: artifact_dir.into(),
            powershell_path: std::env::var("DEVICE_BOT_POWERSHELL_PATH")
                .unwrap_or_else(|_| "powershell.exe".into()),
            active_process_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()))
    }

    fn require_device(&self, device_id: &str) -> Result<(), RobotError> {
        if device_id != DEVICE_ID {
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

    async fn uia_elements(
        &self,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        if max_results == Some(0) {
            return Ok(vec![]);
        }

        self.activate_target().await?;
        let process_id = *self.active_process_id.lock().await;
        let bridge_process = match process_id {
            Some(id) => Some(id),
            None if flutter_bridge::directory().is_dir() => self.foreground_process_id().await.ok(),
            None => None,
        };
        if let Some(id) = bridge_process {
            if let Some(elements) = flutter_bridge::elements(id).await {
                return Ok(apply_element_query(
                    elements,
                    &ElementQuery {
                        element_type: None,
                        filter: filter.map(str::to_owned),
                        max_results,
                    },
                ));
            }
        }
        let script = build_uia_script(process_id, filter, max_results);
        let output = tokio::time::timeout(UIA_QUERY_TIMEOUT, self.powershell(script))
            .await
            .map_err(|_| {
                RobotError::CommandFailed(
                    "Windows UI Automation traversal exceeded its 2.5 second deadline".into(),
                )
            })??;
        parse_uia_elements(&output)
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
        let Some(process_id) = *self.active_process_id.lock().await else {
            return Ok(());
        };
        let script = format!(
            r#"
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotWindow {{
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint source, uint target, bool attach);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr window);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [StructLayout(LayoutKind.Sequential)] private struct MSG {{
    public IntPtr window;
    public uint message;
    public UIntPtr wParam;
    public IntPtr lParam;
    public uint time;
    public int x, y;
    public uint reserved;
  }}
  [DllImport("user32.dll")] private static extern bool PeekMessage(out MSG message, IntPtr window, uint min, uint max, uint remove);
  public static void Activate(IntPtr window) {{
    // Avoid resetting keyboard focus/input state on every query or keystroke
    // when the target is already foreground.
    if (GetForegroundWindow() == window) return;
    MSG queued;
    PeekMessage(out queued, IntPtr.Zero, 0, 0, 0);
    uint ignored;
    uint current = GetCurrentThreadId();
    uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
    uint target = GetWindowThreadProcessId(window, out ignored);
    bool attachedForeground = foreground != 0 && foreground != current && AttachThreadInput(current, foreground, true);
    bool attachedTarget = target != 0 && target != current && target != foreground && AttachThreadInput(current, target, true);
    try {{
      ShowWindow(window, 9);
      BringWindowToTop(window);
      SetForegroundWindow(window);
    }} finally {{
      if (attachedTarget) AttachThreadInput(current, target, false);
      if (attachedForeground) AttachThreadInput(current, foreground, false);
    }}
  }}
}}
'@
$process = Get-Process -Id {process_id} -ErrorAction Stop
$window = $process.MainWindowHandle
if ($window -eq [IntPtr]::Zero) {{ throw "Windows process {process_id} has no main window." }}
[DeviceBotWindow]::Activate($window)
$foregroundPid = [uint32]0
[void][DeviceBotWindow]::GetWindowThreadProcessId([DeviceBotWindow]::GetForegroundWindow(), [ref]$foregroundPid)
if ($foregroundPid -ne {process_id}) {{ throw "Could not activate Windows target {process_id}; foreground process is $foregroundPid. No input was sent." }}
"#
        );
        self.powershell(script).await.map(|_| ())
    }
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

fn build_uia_script(
    process_id: Option<u32>,
    filter: Option<&str>,
    max_results: Option<usize>,
) -> String {
    let mut script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotWindow {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@

$processId = [uint32]__PROCESS_ID__
if ($processId -eq 0) {
  $foreground = [DeviceBotWindow]::GetForegroundWindow()
  if ($foreground -ne [IntPtr]::Zero) {
    [void][DeviceBotWindow]::GetWindowThreadProcessId($foreground, [ref]$processId)
  }
}
if ($processId -eq 0) {
  throw 'The foreground Windows process id is not available.'
}

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition -ArgumentList @(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
  [int]$processId
)
$target = $root.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $condition)
if ($null -eq $target) {
  throw "UI Automation could not find a window for process id $processId. The target may be elevated or running in another desktop session."
}

$walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
$nodes = New-Object 'System.Collections.Generic.List[object]'
$filter = __FILTER__
$maxResults = [int]__MAX_RESULTS__
$maxNodes = [int]__MAX_NODES__
$maxDepth = [int]__MAX_DEPTH__

function Normalize-Text($value) {
  if ($null -eq $value) { return $null }
  $text = [string]$value
  if ([string]::IsNullOrWhiteSpace($text)) { return $null }
  return $text
}

function Get-Role([int]$controlType) {
  switch ($controlType) {
    50000 { return 'button' }
    50002 { return 'checkbox' }
    50003 { return 'combobox' }
    50004 { return 'textField' }
    50005 { return 'link' }
    50006 { return 'image' }
    50007 { return 'listItem' }
    50008 { return 'list' }
    50009 { return 'menu' }
    50011 { return 'menuItem' }
    50012 { return 'progressBar' }
    50013 { return 'radio' }
    50014 { return 'scrollView' }
    50015 { return 'slider' }
    50018 { return 'list' }
    50020 { return 'text' }
    50023 { return 'list' }
    50024 { return 'listItem' }
    50032 { return 'window' }
    default { return 'unknown' }
  }
}

function Add-Element([System.Windows.Automation.AutomationElement]$element, [int]$depth) {
  if ($null -eq $element -or $depth -gt $maxDepth -or $nodes.Count -ge $maxNodes) { return }

  $role = 'unknown'
  $automationId = $null
  $name = $null
  $helpText = $null
  $left = 0
  $top = 0
  $right = 0
  $bottom = 0
  $enabled = $true

  try { $role = Get-Role ([int]$element.Current.ControlType.Id) } catch { }
  try { $automationId = Normalize-Text $element.Current.AutomationId } catch { }
  try { $name = Normalize-Text $element.Current.Name } catch { }
  try { $helpText = Normalize-Text $element.Current.HelpText } catch { }
  try {
    $rectangle = $element.Current.BoundingRectangle
    $left = [int][Math]::Round($rectangle.Left)
    $top = [int][Math]::Round($rectangle.Top)
    $right = [int][Math]::Round($rectangle.Right)
    $bottom = [int][Math]::Round($rectangle.Bottom)
  } catch { }
  try { $enabled = [bool]$element.Current.IsEnabled } catch { }
  $focused = $null
  try { $focused = [bool]$element.Current.HasKeyboardFocus } catch { }
  $checked = $null
  try {
    $toggle = $element.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if ($null -ne $toggle) {
      $checked = ([System.Windows.Automation.TogglePattern]$toggle).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
    }
  } catch { }
  $selected = $null
  try {
    $selection = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    if ($null -ne $selection) {
      $selected = [bool]([System.Windows.Automation.SelectionItemPattern]$selection).Current.IsSelected
    }
  } catch { }
  $nativeValue = $null
  try {
    $valuePattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    if ($null -ne $valuePattern) {
      $nativeValue = Normalize-Text ([System.Windows.Automation.ValuePattern]$valuePattern).Current.Value
    }
  } catch { }

  $matches = [string]::IsNullOrEmpty($filter)
  if (-not $matches) {
    foreach ($field in @($role, $automationId, $name, $helpText)) {
      if ($null -ne $field -and $field.ToLowerInvariant().Contains($filter)) {
        $matches = $true
        break
      }
    }
  }

  if ($matches -and $right -gt $left -and $bottom -gt $top -and $nodes.Count -lt $maxResults) {
    $contentDescription = $helpText
    if ($null -eq $contentDescription) { $contentDescription = $name }
    [void]$nodes.Add([pscustomobject]@{
      className = $role
      resourceId = $automationId
      text = $name
      contentDescription = $contentDescription
      left = $left
      top = $top
      right = $right
      bottom = $bottom
      clickable = @('button', 'link', 'menuItem', 'radio', 'checkbox') -contains $role
      enabled = $enabled
      focused = $focused
      checked = $checked
      selected = $selected
      value = $nativeValue
    })
  }

  if ($nodes.Count -ge $maxResults -or $nodes.Count -ge $maxNodes) { return }
  try {
    $child = $walker.GetFirstChild($element)
    while ($null -ne $child -and $nodes.Count -lt $maxResults -and $nodes.Count -lt $maxNodes) {
      Add-Element $child ($depth + 1)
      $next = $walker.GetNextSibling($child)
      if ($null -eq $next -or $next.Equals($child)) { break }
      $child = $next
    }
  } catch { }
}

Add-Element $target 0
if ($nodes.Count -eq 0) { '[]' } else { $nodes | ConvertTo-Json -Depth 4 -Compress }
"#
    .to_owned();
    script = script.replace("__PROCESS_ID__", &process_id.unwrap_or(0).to_string());
    script = script.replace(
        "__FILTER__",
        &ps_quote(&filter.unwrap_or("").to_lowercase()),
    );
    script = script.replace(
        "__MAX_RESULTS__",
        &max_results
            .unwrap_or(MAX_UI_NODES)
            .min(MAX_UI_NODES)
            .to_string(),
    );
    script = script.replace("__MAX_NODES__", &MAX_UI_NODES.to_string());
    script.replace("__MAX_DEPTH__", &MAX_UI_DEPTH.to_string())
}

fn parse_uia_elements(output: &[u8]) -> Result<Vec<UiElement>, RobotError> {
    let output = output.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(output);
    let value = serde_json::from_slice::<Value>(output).map_err(|error| {
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
                value: optional_string("value"),
                element_type: None,
            })
        })
        .collect()
}

#[async_trait]
impl Robot for WindowsRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        if !cfg!(target_os = "windows") {
            return Ok(vec![]);
        }
        let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "This PC".into());
        Ok(vec![RobotDevice {
            id: DEVICE_ID.into(),
            name,
            platform: Platform::Windows,
            state: DeviceState::Available,
        }])
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        self.require_device(device_id)?;
        let script = r#"
$paths = @(
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$apps = foreach ($path in $paths) {
  Get-ItemProperty $path -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName } |
    Select-Object @{Name='name'; Expression={$_.DisplayName}},
                  @{Name='version'; Expression={$_.DisplayVersion}},
                  @{Name='publisher'; Expression={$_.Publisher}},
                  @{Name='installLocation'; Expression={$_.InstallLocation}}
}
if ($null -eq $apps) {
  '[]'
} else {
  @($apps) | ConvertTo-Json -Depth 3 -Compress
}
"#;
        let output = self.powershell(script.into()).await?;
        let value = serde_json::from_slice::<Value>(&output).map_err(|error| {
            RobotError::CommandFailed(format!("PowerShell returned invalid app JSON: {error}"))
        })?;
        let apps = match value {
            Value::Null => json!([]),
            Value::Array(_) => value,
            other => Value::Array(vec![other]),
        };
        Ok(json!({
            "platform": "windows",
            "deviceId": device_id,
            "apps": apps,
        }))
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        self.activate_target().await?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create screenshot directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "windows-screenshot-{}.png",
            SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
             $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height; \
             $graphics = [System.Drawing.Graphics]::FromImage($bitmap); \
             $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size); \
             $bitmap.Save({}, [System.Drawing.Imaging.ImageFormat]::Png); $graphics.Dispose(); $bitmap.Dispose()",
            ps_quote(&path.to_string_lossy())
        );
        self.powershell(script).await?;
        let image = tokio::fs::read(&path).await.map_err(|error| {
            RobotError::CommandFailed(format!("could not read Windows screenshot: {error}"))
        });
        let _ = tokio::fs::remove_file(path).await;
        image
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.activate_target().await?;
        self.powershell(format!(
            "{}; [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            mouse_helpers(), point.x.round(), point.y.round()
        )).await.map(|_| ())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        if app_id.trim().is_empty() {
            return Err(RobotError::InvalidInput("app id must not be empty".into()));
        }
        let output = self
            .powershell(format!(
                "$target = {}; $name = [System.IO.Path]::GetFileNameWithoutExtension($target); $process = Get-Process -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1; if ($null -eq $process) {{ $process = Start-Process -FilePath $target -PassThru }}; if ($null -eq $process) {{ throw \"could not start $target\" }}; try {{ $process.WaitForInputIdle(5000) | Out-Null }} catch {{}}; [Console]::WriteLine([int]$process.Id)",
                ps_quote(app_id)
            ))
            .await?;
        let process_id = parse_process_id(&output)?;
        *self.active_process_id.lock().await = Some(process_id);
        Ok(())
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        _app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _ = url;
        Err(RobotError::Unsupported(
            "URL navigation belongs to the Chrome DevTools robot; use a chrome:<target-id> device"
                .into(),
        ))
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.activate_target().await?;
        let script = r#"
Add-Type @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class DeviceBotKeyboard {
  [StructLayout(LayoutKind.Sequential)] private struct KEYBDINPUT {
    public ushort key, scan;
    public uint flags, time;
    public UIntPtr extra;
  }
  [StructLayout(LayoutKind.Sequential)] private struct MOUSEINPUT {
    public int x, y;
    public uint data, flags, time;
    public UIntPtr extra;
  }
  [StructLayout(LayoutKind.Explicit)] private struct INPUTDATA {
    [FieldOffset(0)] public KEYBDINPUT keyboard;
    [FieldOffset(0)] public MOUSEINPUT mouse;
  }
  [StructLayout(LayoutKind.Sequential)] private struct INPUT {
    public uint type;
    public INPUTDATA data;
  }
  [DllImport("user32.dll", SetLastError=true)] private static extern uint SendInput(uint count, INPUT[] input, int size);
  public static void TypeLiteral(string text) {
    var input = new List<INPUT>();
    foreach (char character in text) {
      if (character == '\r') continue;
      // Unicode packets preserve literal text despite keyboard layout, dead
      // keys, or a Telex input method. Newlines retain the existing Enter behavior.
      bool enter = character == '\n';
      var key = new KEYBDINPUT {
        key = enter ? (ushort)13 : (ushort)0,
        scan = enter ? (ushort)0 : character,
        flags = enter ? 0u : 4u
      };
      input.Add(new INPUT { type=1, data=new INPUTDATA { keyboard=key } });
      key.flags |= 2;
      input.Add(new INPUT { type=1, data=new INPUTDATA { keyboard=key } });
    }
    if (input.Count == 0) return;
    var keys = input.ToArray();
    if (SendInput((uint)keys.Length, keys, Marshal.SizeOf(typeof(INPUT))) != keys.Length) {
      throw new Win32Exception(Marshal.GetLastWin32Error(), "Windows could not insert the literal text");
    }
  }
}
'@
[DeviceBotKeyboard]::TypeLiteral(__TEXT__)
"#.replace("__TEXT__", &ps_quote(text));
        self.powershell(script).await.map(|_| ())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.activate_target().await?;
        if (end.y - start.y).abs() > (end.x - start.x).abs() {
            let process_id = self.target_process_id().await?;
            if flutter_bridge::action(
                process_id,
                json!({
                    "command": "swipe",
                    "startX": start.x, "startY": start.y,
                    "endX": end.x, "endY": end.y,
                    "durationMs": duration_ms.unwrap_or(300).min(2000),
                }),
            )
            .await?
            {
                return Ok(());
            }
        }
        let steps = 12_u64;
        let delay = duration_ms.unwrap_or(300) / steps;
        self.powershell(format!(
            "{}; [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); for ($step = 1; $step -le {}; $step++) {{ [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point([Math]::Round({} + ({} - {}) * $step / {}), [Math]::Round({} + ({} - {}) * $step / {})); Start-Sleep -Milliseconds {} }}; [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            mouse_helpers(), start.x.round(), start.y.round(), steps, start.x, end.x, start.x, steps, start.y, end.y, start.y, steps, delay
        )).await.map(|_| ())
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.activate_target().await?;
        let process_id = self.target_process_id().await?;
        if flutter_bridge::action(
            process_id,
            json!({
                "command": "imeAction", "action": action.as_str(),
            }),
        )
        .await?
        {
            Ok(())
        } else {
            Err(RobotError::Unsupported(
                "Windows IME actions require the Flutter widget bridge".into(),
            ))
        }
    }

    async fn install_app(&self, device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app installation is not supported by the Windows robot".into(),
        ))
    }

    async fn uninstall_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app removal is not supported by the Windows robot".into(),
        ))
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        self.uia_elements(None, None).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        self.uia_elements(filter, max_results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_literals_are_quoted() {
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn parses_process_id_output() {
        assert_eq!(parse_process_id(b"\r\n1234\r\n").unwrap(), 1234);
        assert_eq!(parse_process_id("\u{feff}5678\n".as_bytes()).unwrap(), 5678);
        assert!(parse_process_id(b"not-a-process").is_err());
    }

    #[test]
    fn uia_script_uses_raw_view_and_process_condition() {
        let script = build_uia_script(Some(1234), None, None);
        assert!(script.contains("UIAutomationClient"));
        assert!(script.contains("TreeWalker]::RawViewWalker"));
        assert!(script.contains("ProcessIdProperty"));
        assert!(script.contains("[uint32]1234"));
    }

    #[test]
    fn uia_script_embeds_case_insensitive_filter_and_bounded_results() {
        let script = build_uia_script(None, Some("Save\' As"), Some(20));
        assert!(script.contains("$filter = 'save'' as'"));
        assert!(script.contains("$maxResults = [int]20"));
        assert!(script.contains("$maxNodes = [int]5000"));
        assert!(script.contains("$maxDepth = [int]128"));
    }

    #[test]
    fn parses_uia_payload_into_portable_elements() {
        let elements = parse_uia_elements(
            br#"[{"className":"button","resourceId":"save","text":"Save","contentDescription":"Save","left":10,"top":20,"right":70,"bottom":50,"clickable":true,"enabled":false}]"#,
        )
        .unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].class_name, "button");
        assert_eq!(elements[0].resource_id.as_deref(), Some("save"));
        assert_eq!(
            elements[0].bounds,
            Bounds {
                left: 10,
                top: 20,
                right: 70,
                bottom: 50
            }
        );
        assert!(elements[0].clickable);
        assert!(!elements[0].enabled);
    }

    #[test]
    fn parses_uia_payload_with_a_utf8_bom() {
        let mut output = vec![0xef, 0xbb, 0xbf];
        output.extend_from_slice(br#"[]"#);
        assert!(parse_uia_elements(&output).unwrap().is_empty());
    }

    #[tokio::test]
    async fn zero_max_results_does_not_start_powershell() {
        let robot = WindowsRobot::new("artifacts");
        assert!(robot.uia_elements(None, Some(0)).await.unwrap().is_empty());
    }

    // Run against a started, visible examples/flutter_app Windows build:
    // DEVICE_BOT_FLUTTER_SMOKE_PID=<pid> cargo test -p robot-windows flutter_live -- --ignored
    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires a running Flutter sample and an interactive desktop"]
    async fn flutter_live_widget_tree_and_native_input() {
        let process_id: u32 = std::env::var("DEVICE_BOT_FLUTTER_SMOKE_PID")
            .expect("set DEVICE_BOT_FLUTTER_SMOKE_PID to the running sample PID")
            .parse()
            .unwrap();
        let robot = WindowsRobot::new("artifacts");
        *robot.active_process_id.lock().await = Some(process_id);
        let query = ElementQuery {
            element_type: Some(robot_core::ElementType::TextField),
            filter: Some("email-field".into()),
            max_results: Some(1),
        };
        let elements = robot
            .list_elements_query(DEVICE_ID, query.clone())
            .await
            .unwrap();
        assert_eq!(elements.len(), 1);
        let email = &elements[0];
        assert_eq!(email.resource_id.as_deref(), Some("email-field"));
        robot
            .tap(
                DEVICE_ID,
                Point {
                    x: f64::from(email.bounds.left + email.bounds.right) / 2.0,
                    y: f64::from(email.bounds.top + email.bounds.bottom) / 2.0,
                },
            )
            .await
            .unwrap();
        let literal = "+rust Ỏbit123! ^%{}[]  ";
        robot.type_text(DEVICE_ID, literal).await.unwrap();
        let updated = robot.list_elements_query(DEVICE_ID, query).await.unwrap();
        assert_eq!(updated[0].focused, Some(true));
        assert!(updated[0].value.as_deref().unwrap().contains(literal));
        // An absent bridge must leave native UIA usable for this same target.
        let manifest = flutter_bridge::directory().join(format!("{process_id}.json"));
        let saved = manifest.with_extension("smoke-backup");
        tokio::fs::rename(&manifest, &saved).await.unwrap();
        let native = robot.list_elements(DEVICE_ID).await;
        tokio::fs::rename(&saved, &manifest).await.unwrap();
        assert!(native.is_ok(), "native UIA fallback failed: {native:?}");
    }
}

//! Native Windows desktop driver backed by PowerShell and WinForms.
//!
//! The process must run in an interactive Windows session. Desktop input can
//! be blocked by UAC's secure desktop or by a policy that disallows UI access.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use mobile_robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use serde_json::{Value, json};
use tokio::process::Command;

const DEVICE_ID: &str = "windows-host";
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct WindowsRobot {
    artifact_dir: PathBuf,
    powershell_path: String,
}

impl WindowsRobot {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifact_dir: artifact_dir.into(),
            powershell_path: "powershell.exe".into(),
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
        let output = Command::new(&self.powershell_path)
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
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
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn send_keys_literal(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '+' | '^' | '%' | '~' | '(' | ')' | '[' | ']' => format!("{{{character}}}"),
            '{' => "{{}".into(),
            '}' => "{}}".into(),
            '\n' => "{ENTER}".into(),
            '\r' => String::new(),
            _ => character.to_string(),
        })
        .collect()
}

fn mouse_helpers() -> &'static str {
    "Add-Type @'\nusing System;\nusing System.Runtime.InteropServices;\npublic static class MobileMcpMouse {\n  [DllImport(\"user32.dll\")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);\n}\n'@\nAdd-Type -AssemblyName System.Windows.Forms\nAdd-Type -AssemblyName System.Drawing"
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
        self.powershell(format!(
            "{}; [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            mouse_helpers(), point.x.round(), point.y.round()
        )).await.map(|_| ())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.powershell(format!("Start-Process -FilePath {}", ps_quote(app_id)))
            .await
            .map(|_| ())
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
        self.powershell(format!(
            "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait({})",
            ps_quote(&send_keys_literal(text))
        )).await.map(|_| ())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let steps = 12_u64;
        let delay = duration_ms.unwrap_or(300) / steps;
        self.powershell(format!(
            "{}; [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); for ($step = 1; $step -le {}; $step++) {{ [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point([Math]::Round({} + ({} - {}) * $step / {}), [Math]::Round({} + ({} - {}) * $step / {})); Start-Sleep -Milliseconds {} }}; [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            mouse_helpers(), start.x.round(), start.y.round(), steps, start.x, end.x, start.x, steps, start.y, end.y, start.y, steps, delay
        )).await.map(|_| ())
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
        Err(RobotError::Unsupported(
            "accessibility-tree discovery is not supported by the Windows robot".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_literals_are_quoted_and_sendkeys_is_escaped() {
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
        assert_eq!(send_keys_literal("a+b\n"), "a{+}b{ENTER}");
    }
}

use super::*;
use robot_core::{DesktopDisplay, ScreenshotOptions};

const WINDOWS: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotWindow {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr window);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint source, uint target, bool attach);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("dwmapi.dll")] public static extern int DwmFlush();
  [StructLayout(LayoutKind.Sequential)] private struct MSG {
    public IntPtr window; public uint message; public UIntPtr wParam;
    public IntPtr lParam; public uint time; public int x, y; public uint reserved;
  }
  [DllImport("user32.dll")] private static extern bool PeekMessage(out MSG message, IntPtr window, uint min, uint max, uint remove);
  public static void Activate(IntPtr window) {
    if (window == IntPtr.Zero || !IsWindow(window)) throw new Exception("The target window no longer exists.");
    if (GetForegroundWindow() == window) return;
    MSG queued; PeekMessage(out queued, IntPtr.Zero, 0, 0, 0);
    uint ignored, current = GetCurrentThreadId();
    uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
    uint target = GetWindowThreadProcessId(window, out ignored);
    bool attachedForeground = foreground != 0 && foreground != current && AttachThreadInput(current, foreground, true);
    bool attachedTarget = target != 0 && target != current && target != foreground && AttachThreadInput(current, target, true);
    try { ShowWindow(window, 9); BringWindowToTop(window); SetForegroundWindow(window); }
    finally {
      if (attachedTarget) AttachThreadInput(current, target, false);
      if (attachedForeground) AttachThreadInput(current, foreground, false);
    }
    for (int i=0; i<40; i++) {
      if (GetForegroundWindow() == window) { DwmFlush(); return; }
      System.Threading.Thread.Sleep(25);
    }
    throw new Exception("Could not focus the requested window.");
  }
}
'@
try { [void][DeviceBotWindow]::SetProcessDpiAwarenessContext([IntPtr](-4)) }
catch { [void][DeviceBotWindow]::SetProcessDPIAware() }
"#;

pub(super) fn activate_script(process_id: u32) -> String {
    format!(
        "{WINDOWS}\n$process = Get-Process -Id {process_id} -ErrorAction Stop\n[DeviceBotWindow]::Activate($process.MainWindowHandle)\n$foregroundPid = [uint32]0\n[void][DeviceBotWindow]::GetWindowThreadProcessId([DeviceBotWindow]::GetForegroundWindow(), [ref]$foregroundPid)\nif ($foregroundPid -ne {process_id}) {{ throw 'Target activation failed; no input was sent.' }}"
    )
}

fn capture_script(
    path: &std::path::Path,
    process_id: Option<u32>,
    options: &ScreenshotOptions,
) -> String {
    let app = options
        .app_id
        .as_deref()
        .map(ps_quote)
        .unwrap_or_else(|| "$null".into());
    let display = options
        .display_id
        .as_deref()
        .map(ps_quote)
        .unwrap_or_else(|| "$null".into());
    format!(
        r#"{WINDOWS}
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$previousWindow = [DeviceBotWindow]::GetForegroundWindow()
$targetPid = [uint32]{pid}
$app = {app}
$displayId = {display}
$bitmap = $null; $graphics = $null
try {{
  if ($null -ne $app) {{
    $name = [System.IO.Path]::GetFileNameWithoutExtension($app)
    $process = Get-Process -Name $name -ErrorAction SilentlyContinue | Where-Object {{ $_.MainWindowHandle -ne [IntPtr]::Zero }} | Select-Object -First 1
    if ($null -eq $process) {{ $process = Start-Process -FilePath $app -PassThru }}
    if ($null -eq $process) {{ throw 'Could not start the target app.' }}
    try {{ [void]$process.WaitForInputIdle(5000) }} catch {{}}
    $targetPid = [uint32]$process.Id
  }}
  if ($targetPid -ne 0) {{
    $process = Get-Process -Id $targetPid -ErrorAction Stop
    $process.Refresh()
    [DeviceBotWindow]::Activate($process.MainWindowHandle)
  }}
  $screen = if ($null -eq $displayId) {{ [System.Windows.Forms.Screen]::PrimaryScreen }} else {{ [System.Windows.Forms.Screen]::AllScreens | Where-Object {{ $_.DeviceName -eq $displayId }} | Select-Object -First 1 }}
  if ($null -eq $screen) {{ throw 'The selected display is no longer connected; refresh displays.' }}
  $bounds = $screen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  if ($targetPid -ne 0) {{
    $foregroundPid = [uint32]0
    [void][DeviceBotWindow]::GetWindowThreadProcessId([DeviceBotWindow]::GetForegroundWindow(), [ref]$foregroundPid)
    if ($foregroundPid -ne $targetPid) {{ throw 'Target lost focus; screenshot was not taken.' }}
  }}
  [void][DeviceBotWindow]::DwmFlush()
  $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
  $bitmap.Save({path}, [System.Drawing.Imaging.ImageFormat]::Png)
  [Console]::WriteLine($targetPid)
}} finally {{
  if ($null -ne $graphics) {{ $graphics.Dispose() }}
  if ($null -ne $bitmap) {{ $bitmap.Dispose() }}
  if ($previousWindow -ne [IntPtr]::Zero) {{ [DeviceBotWindow]::Activate($previousWindow) }}
}}
"#,
        pid = process_id.unwrap_or(0),
        path = ps_quote(&path.to_string_lossy())
    )
}

impl WindowsRobot {
    pub(super) async fn desktop_displays(&self) -> Result<Vec<DesktopDisplay>, RobotError> {
        let output = self.powershell(format!(r#"{WINDOWS}
Add-Type -AssemblyName System.Windows.Forms
$screens = @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object {{
  @{{id=$_.DeviceName; name=$_.DeviceName; isPrimary=$_.Primary;
    bounds=@{{left=$_.Bounds.Left; top=$_.Bounds.Top; right=$_.Bounds.Right; bottom=$_.Bounds.Bottom}} }}
}})
ConvertTo-Json -InputObject $screens -Depth 4 -Compress
"#)).await?;
        serde_json::from_slice(output.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&output)).map_err(
            |error| RobotError::CommandFailed(format!("invalid Windows display list: {error}")),
        )
    }

    pub(super) async fn capture_desktop(
        &self,
        options: &ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        let _guard = self.capture_lock.lock().await;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create screenshot directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "windows-screenshot-{}.png",
            SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let script = capture_script(&path, *self.active_process_id.lock().await, options);
        let image = async {
            let output = self.powershell(script).await?;
            if let Ok(pid) = parse_process_id(&output) {
                *self.active_process_id.lock().await = Some(pid);
            }
            tokio::fs::read(&path).await.map_err(|error| {
                RobotError::CommandFailed(format!("could not read Windows screenshot: {error}"))
            })
        }
        .await;
        let _ = tokio::fs::remove_file(path).await;
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_is_one_transaction_with_display_bounds_and_focus_restoration() {
        let script = capture_script(
            std::path::Path::new("capture.png"),
            Some(123),
            &ScreenshotOptions {
                app_id: Some("O'Brien.exe".into()),
                display_id: Some(r"\\.\DISPLAY2".into()),
            },
        );
        assert!(script.contains("$app = 'O''Brien.exe'"));
        assert!(script.contains(r"$displayId = '\\.\DISPLAY2'"));
        let save_previous = script.find("$previousWindow =").unwrap();
        let activate = script
            .find("[DeviceBotWindow]::Activate($process.MainWindowHandle)")
            .unwrap();
        let capture = script
            .find("$graphics.CopyFromScreen($bounds.Location")
            .unwrap();
        let restore = script
            .find("[DeviceBotWindow]::Activate($previousWindow)")
            .unwrap();
        assert!(save_previous < activate && activate < capture && capture < restore);
        assert!(script.contains("} finally {"));
        assert!(!script.contains("PrimaryScreen.Bounds"));
    }
}

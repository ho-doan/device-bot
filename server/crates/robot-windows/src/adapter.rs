//! Portable device operations backed by native platform modules.

use std::sync::Arc;

use async_trait::async_trait;
use robot_core::{
    AppWindow, DeviceState, ElementQuery, ImeAction, Platform, Point, Robot, RobotDevice,
    RobotError, RunningApp, UiElement, UiTarget,
};
use serde_json::{Value, json};

use super::{
    DEVICE_ID, WindowsRobot, flutter_bridge, live, mouse_helpers, parse_powershell_json,
    parse_process_id, ps_quote,
};

#[async_trait]
impl Robot for WindowsRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        self.require_device(id)?;
        let mut report = robot_core::CapabilityReport::new(id);
        report.capabilities.discovery =
            robot_core::Capability::available("Windows host driver is present");
        report.capabilities.observation = robot_core::Capability::unsupported(
            "Native application log and network observation is not implemented",
        );
        #[cfg(target_os = "windows")]
        {
            #[link(name = "user32")]
            unsafe extern "system" {
                fn OpenInputDesktop(flags: u32, inherit: i32, access: u32)
                -> *mut std::ffi::c_void;
                fn CloseDesktop(desktop: *mut std::ffi::c_void) -> i32;
            }
            let interactive = {
                let desktop = unsafe { OpenInputDesktop(0, 0, 0x0001) };
                if desktop.is_null() {
                    false
                } else {
                    unsafe {
                        CloseDesktop(desktop);
                    }
                    true
                }
            };
            if !interactive {
                let blocked = robot_core::Capability::blocked(
                    "Start the server in an accessible interactive Windows desktop session",
                );
                report.capabilities.input = blocked.clone();
                report.capabilities.elements = blocked.clone();
                report.capabilities.screenshot = blocked.clone();
                report.capabilities.screen_recording = blocked;
            } else {
                let uia = if id == DEVICE_ID {
                    self.uia_elements(Some(1)).await.is_ok()
                } else {
                    self.live_elements(id, Some(1)).await.is_ok()
                };
                let ui_status = if uia {
                    robot_core::Capability::available(
                        "Interactive desktop and UI Automation query are responsive; later input can fail",
                    )
                } else {
                    robot_core::Capability::blocked(
                        "Select an accessible foreground window and check UI Automation",
                    )
                };
                report.capabilities.input = ui_status.clone();
                report.capabilities.elements = ui_status;
                let displays = self.desktop_displays().await.is_ok();
                if displays {
                    report.capabilities.screenshot = robot_core::Capability::available(
                        "Interactive desktop and display enumeration are responsive; capture can fail",
                    );
                } else {
                    report.capabilities.screenshot = robot_core::Capability::blocked(
                        "Connect an active display to the Windows desktop session",
                    );
                }
                let ffmpeg = live::resolve_ffmpeg(
                    std::env::var("DEVICE_BOT_FFMPEG_PATH").ok(),
                    std::env::current_exe().ok().as_deref(),
                );
                let probe = |argument: &'static str| async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        tokio::process::Command::new(&ffmpeg)
                            .args(["-hide_banner", argument])
                            .creation_flags(0x08000000)
                            .kill_on_drop(true)
                            .output(),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .filter(|output| output.status.success())
                    .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
                };
                let (devices, encoders) = tokio::join!(probe("-devices"), probe("-encoders"));
                report.capabilities.screen_recording = if displays
                    && devices.as_deref().is_some_and(|s| s.contains("gdigrab"))
                    && encoders.as_deref().is_some_and(|s| s.contains("libx264"))
                {
                    robot_core::Capability::available(
                        "Interactive desktop, display and ffmpeg gdigrab/libx264 support are present; recording can fail",
                    )
                } else {
                    robot_core::Capability::blocked(
                        "Connect a display and install ffmpeg with gdigrab and libx264 support",
                    )
                };
            }
        }
        Ok(report)
    }

    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        self.require_device(id)?;
        let _input = self.capture_lock.lock().await;
        if !dx.is_finite()
            || !dy.is_finite()
            || dx.abs() > i32::MAX as f64
            || dy.abs() > i32::MAX as f64
        {
            return Err(RobotError::InvalidInput(
                "Scroll deltas must be finite 32-bit values.".into(),
            ));
        }
        let prefix = self.live_input_prefix(id, &[at]).await?;
        if prefix.is_empty() {
            self.activate_target().await?;
        }
        let vertical = (-dy.round() as i32) as u32;
        let horizontal = (dx.round() as i32) as u32;
        self.powershell(format!("{}\n{prefix}\n[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0800, 0, 0, {vertical}, [UIntPtr]::Zero); [MobileMcpMouse]::mouse_event(0x1000, 0, 0, {horizontal}, [UIntPtr]::Zero)", mouse_helpers(), at.x.round(), at.y.round())).await.map(|_| ())
    }

    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        self.live_context(id).await
    }

    async fn start_video_stream(&self, id: &str) -> Result<robot_core::VideoStream, RobotError> {
        self.start_live_video(id).await
    }

    fn fork_session(&self) -> Option<Arc<dyn Robot>> {
        let mut session = self.clone();
        session.active_process_id = Arc::default();
        session.active_window_id = Arc::default();
        session.live_geometry = Arc::default();
        session.recording = Arc::default();
        Some(Arc::new(session))
    }

    async fn close_session(&self) -> Result<(), RobotError> {
        self.recording.lock().await.close().await
    }

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

    async fn list_running_apps(&self, device_id: &str) -> Result<Vec<RunningApp>, RobotError> {
        self.require_device(device_id)?;
        let windows = self.listed_windows(None).await?;
        let mut apps = Vec::new();
        for window in &windows {
            if apps
                .iter()
                .any(|app: &RunningApp| app.process_id == window.process_id)
            {
                continue;
            }
            apps.push(RunningApp {
                process_id: window.process_id,
                name: window.name.clone(),
                bundle_id: None,
            });
        }
        apps.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then(left.process_id.cmp(&right.process_id))
        });
        Ok(apps)
    }

    async fn list_app_windows(
        &self,
        device_id: &str,
        process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        self.require_device(device_id)?;
        Ok(self
            .listed_windows(process_id)
            .await?
            .into_iter()
            .map(|window| AppWindow {
                window_id: window.window_id,
                process_id: window.process_id,
                title: if window.title.is_empty() {
                    window.name
                } else {
                    window.title
                },
                focused: false,
            })
            .collect())
    }

    async fn select_ui_target(&self, device_id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return Err(RobotError::InvalidInput(
                "A Windows Live target is immutable; select another window on windows-host.".into(),
            ));
        }
        let pid = target.process_id.or(*self.active_process_id.lock().await);
        let selected_window = if let Some(window) = target.window_id.as_deref() {
            let pid = pid.ok_or_else(|| {
                RobotError::InvalidInput(
                    "Select the owning process before selecting a window.".into(),
                )
            })?;
            let id = format!("windows:{pid}:{window}");
            let selected = live::Target::parse(&id)?.expect("Windows target prefix");
            self.live_geometry_for(selected).await?;
            Some(selected.hwnd)
        } else {
            None
        };
        if let Some(process_id) = target.process_id {
            if process_id == 0 {
                return Err(RobotError::InvalidInput(
                    "processId must be a positive 32-bit integer".into(),
                ));
            }
            *self.active_process_id.lock().await = Some(process_id);
        }
        if self.active_process_id.lock().await.is_none() {
            return Err(RobotError::InvalidInput(
                "select a running app before querying elements".into(),
            ));
        }
        *self.active_window_id.lock().await = selected_window;
        Ok(())
    }

    async fn activate_ui_target(
        &self,
        device_id: &str,
        target: UiTarget,
    ) -> Result<(), RobotError> {
        let _input = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            self.require_device(device_id)?;
            return self
                .powershell(self.live_input_prefix(device_id, &[]).await?)
                .await
                .map(|_| ());
        }
        self.select_ui_target(device_id, target).await?;
        self.activate_target().await
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
        let value = parse_powershell_json(&output).map_err(|error| {
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

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        let ffmpeg = live::resolve_ffmpeg(
            std::env::var("DEVICE_BOT_FFMPEG_PATH").ok(),
            std::env::current_exe().ok().as_deref(),
        );
        self.recording
            .lock()
            .await
            .start(&ffmpeg, &self.artifact_dir)
            .await
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        self.recording.lock().await.stop().await
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        self.capture_desktop(&robot_core::ScreenshotOptions::default())
            .await
    }

    async fn take_screenshot_with_options(
        &self,
        device_id: &str,
        options: &robot_core::ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        self.capture_desktop(options).await
    }

    async fn list_displays(
        &self,
        device_id: &str,
    ) -> Result<Vec<robot_core::DesktopDisplay>, RobotError> {
        self.require_device(device_id)?;
        self.desktop_displays().await
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _input = self.capture_lock.lock().await;
        let prefix = self.live_input_prefix(device_id, &[point]).await?;
        if prefix.is_empty() {
            self.activate_target().await?;
        }
        self.powershell(format!(
            "{}; {prefix}\n[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            mouse_helpers(), point.x.round(), point.y.round()
        )).await.map(|_| ())
    }

    async fn pointer_gesture(
        &self,
        device_id: &str,
        point: Point,
        gesture: robot_core::PointerGesture,
        context: Option<&robot_core::WebContext>,
    ) -> Result<(), RobotError> {
        use robot_core::PointerGesture::*;
        if gesture == Tap {
            return self.tap_in_context(device_id, point, context).await;
        }
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "Windows DOM pointer gestures".into(),
            ));
        }
        self.require_device(device_id)?;
        if ![point.x, point.y]
            .into_iter()
            .all(|value| value.is_finite() && (i32::MIN as f64..=i32::MAX as f64).contains(&value))
        {
            return Err(RobotError::InvalidInput(
                "Invalid Windows pointer coordinates".into(),
            ));
        }
        let guard = self.capture_lock.clone().lock_owned().await;
        let prefix = self.live_input_prefix(device_id, &[point]).await?;
        if prefix.is_empty() {
            self.activate_target().await?;
        }
        let down = "[MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)";
        let up = "[MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)";
        let operation = match gesture {
            Tap => unreachable!(),
            LongPress { duration_ms } => format!(
                "{down}; try {{ Start-Sleep -Milliseconds {duration_ms} }} finally {{ {up} }}"
            ),
            DoubleTap => {
                let check = if prefix.is_empty() {
                    ""
                } else {
                    "if ([DeviceBotLive]::GetForegroundWindow() -ne $target) { throw 'Target lost focus during double tap.' }; [DeviceBotLive]::CheckPoint($target, [System.Windows.Forms.Cursor]::Position.X, [System.Windows.Forms.Cursor]::Position.Y);"
                };
                format!("{down}; {up}; Start-Sleep -Milliseconds 40; {check} {down}; {up}")
            }
        };
        let script = format!(
            "{}\n{prefix}\n[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); {operation}",
            mouse_helpers(),
            point.x.round(),
            point.y.round(),
        );
        // The helper must finish its mouse-up even if the request is cancelled.
        let this = self.clone();
        tokio::spawn(async move {
            let _guard = guard;
            this.powershell(script).await.map(|_| ())
        })
        .await
        .map_err(|error| {
            RobotError::CommandFailed(format!("Windows gesture task failed: {error}"))
        })?
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _input = self.capture_lock.lock().await;
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
        *self.active_window_id.lock().await = None;
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
        let _input = self.capture_lock.lock().await;
        let prefix = self.live_input_prefix(device_id, &[]).await?;
        if prefix.is_empty() {
            self.activate_target().await?;
        }
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
        self.powershell(script.replacen(
            "[DeviceBotKeyboard]::TypeLiteral(",
            &format!("{prefix}\n[DeviceBotKeyboard]::TypeLiteral("),
            1,
        ))
        .await
        .map(|_| ())
    }

    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _input = self.capture_lock.lock().await;
        let selected_id = if device_id == DEVICE_ID {
            let pid = *self.active_process_id.lock().await;
            let hwnd = *self.active_window_id.lock().await;
            let (Some(pid), Some(hwnd)) = (pid, hwnd) else {
                return Err(RobotError::InvalidInput(
                    "Select an app window before clearing text.".into(),
                ));
            };
            format!("windows:{pid}:{hwnd}")
        } else {
            device_id.to_owned()
        };
        let prefix = self.live_input_prefix(&selected_id, &[]).await?;
        self.powershell(live::clear_text_script(&prefix))
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
        self.require_device(device_id)?;
        let _input = self.capture_lock.lock().await;
        let prefix = self.live_input_prefix(device_id, &[start, end]).await?;
        if prefix.is_empty() {
            self.activate_target().await?;
        }
        if prefix.is_empty() && (end.y - start.y).abs() > (end.x - start.x).abs() {
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
        let guard = if prefix.is_empty() {
            ""
        } else {
            "if ([DeviceBotLive]::GetForegroundWindow() -ne $target) { throw 'Target lost focus during drag.' }; [DeviceBotLive]::CheckPoint($target, [System.Windows.Forms.Cursor]::Position.X, [System.Windows.Forms.Cursor]::Position.Y); "
        };
        let steps = 12_u64;
        let delay = duration_ms.unwrap_or(300) / steps;
        self.powershell(format!(
            "{}; {prefix}\n[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); [MobileMcpMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); try {{ for ($step = 1; $step -le {}; $step++) {{ {guard}[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point([Math]::Round({} + ({} - {}) * $step / {}), [Math]::Round({} + ({} - {}) * $step / {})); Start-Sleep -Milliseconds {} }} }} finally {{ [MobileMcpMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero) }}",
            mouse_helpers(), start.x.round(), start.y.round(), steps, start.x, end.x, start.x, steps, start.y, end.y, start.y, steps, delay
        )).await.map(|_| ())
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _input = self.capture_lock.lock().await;
        if live::Target::parse(device_id)?.is_some() {
            return Err(RobotError::Unsupported(
                "Windows Live IME actions require a window-scoped Flutter bridge".into(),
            ));
        }
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

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        if live::Target::parse(device_id)?.is_some() {
            return Ok(vec![]);
        }
        if let Some(elements) = robot_flutter_bridge::configured(device_id).await {
            return Ok(elements);
        }
        let id = self.target_process_id().await?;
        Ok(robot_flutter_bridge::for_process(id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_query(device_id, ElementQuery::default())
            .await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.list_elements_query(
            device_id,
            ElementQuery {
                filter: filter.map(str::to_owned),
                max_results,
                ..Default::default()
            },
        )
        .await
    }

    async fn list_elements_query(
        &self,
        device_id: &str,
        query: ElementQuery,
    ) -> Result<Vec<UiElement>, RobotError> {
        query.reject_keyboard_exclusion()?;
        query.reject_web_context(device_id)?;
        self.require_device(device_id)?;
        self.query_native_elements(device_id, query).await
    }
}

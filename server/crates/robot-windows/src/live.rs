//! HWND-bound FFmpeg capture feeding the shared H.264/WebRTC gateway.
use super::*;
use robot_core::{VideoFrame, VideoStream};
use tokio::io::AsyncReadExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub pid: u32,
    pub hwnd: i64,
}
impl Target {
    pub fn parse(id: &str) -> Result<Option<Self>, RobotError> {
        let Some(rest) = id.strip_prefix("windows:") else {
            return Ok(None);
        };
        let mut parts = rest.split(':');
        let pid = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0);
        let hwnd = parts
            .next()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| *v > 0);
        match (pid, hwnd, parts.next()) {
            (Some(pid), Some(hwnd), None) => Ok(Some(Self { pid, hwnd })),
            _ => Err(RobotError::InvalidInput(
                "Windows Live device must be windows:<positive-pid>:<positive-hwnd>".into(),
            )),
        }
    }
    fn script(self) -> String {
        format!(
            "{HELPERS}\n$target = [IntPtr]([int64]{hwnd})\n[DeviceBotLive]::Validate($target, [uint32]{pid})\n",
            hwnd = self.hwnd,
            pid = self.pid
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

// Kept in one PowerShell process with the action so helper consoles cannot steal
// focus between validation and input. Physical pixels match UIA and gdigrab.
const HELPERS: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotLive {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int x, y; public Point(int x, int y) { this.x=x; this.y=y; } }
  [StructLayout(LayoutKind.Sequential)] private struct Message { public IntPtr window; public uint message; public UIntPtr wParam; public IntPtr lParam; public uint time; public int x,y; public uint reserved; }
  [DllImport("user32.dll")] private static extern bool PeekMessage(out Message m, IntPtr h, uint min, uint max, uint flags);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref Point p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);
  [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent, IntPtr child);
  public static void Validate(IntPtr h, uint expected) {
    uint pid;
    if (!IsWindow(h) || GetWindowThreadProcessId(h, out pid)==0 || pid!=expected)
      throw new Exception("The selected Windows window no longer belongs to the selected app. Reopen Live.");
    if (!IsWindowVisible(h) || IsIconic(h)) throw new Exception("Restore the selected window before using Live.");
  }
  public static int[] Geometry(IntPtr h) {
    Rect r; var p = new Point(0,0);
    if (!GetClientRect(h, out r) || !ClientToScreen(h, ref p) || r.right<2 || r.bottom<2)
      throw new Exception("The selected window has no capturable client area.");
    return new int[] { p.x,p.y,r.right-r.left,r.bottom-r.top };
  }
  public static void Focus(IntPtr h) {
    Message queued; PeekMessage(out queued,IntPtr.Zero,0,0,0);
    uint ignored; uint current=GetCurrentThreadId();
    uint foreground=GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
    uint target=GetWindowThreadProcessId(h, out ignored);
    bool a=foreground!=0 && foreground!=current && AttachThreadInput(current,foreground,true);
    bool b=target!=0 && target!=current && target!=foreground && AttachThreadInput(current,target,true);
    try { BringWindowToTop(h); SetForegroundWindow(h); }
    finally { if(b) AttachThreadInput(current,target,false); if(a) AttachThreadInput(current,foreground,false); }
    if(GetForegroundWindow()!=h) throw new Exception("Could not focus the selected window; input was not sent.");
  }
  public static void CheckPoint(IntPtr h, int x, int y) {
    var g=Geometry(h);
    if(x<g[0] || y<g[1] || x>=g[0]+g[2] || y>=g[1]+g[3])
      throw new Exception("Input is outside the selected window.");
    var hit=WindowFromPoint(new Point(x,y));
    if(hit!=h && !IsChild(h,hit)) throw new Exception("Another window covers the input point; input was not sent.");
  }
}
'@
[void][DeviceBotLive]::SetProcessDpiAwarenessContext([IntPtr](-4))
"#;

fn failure(e: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("Windows Live: {e}"))
}

pub(super) fn clear_text_script(prefix: &str) -> String {
    let helpers = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class DeviceBotClearText {
  [StructLayout(LayoutKind.Sequential)] private struct KEYBDINPUT { public ushort key, scan; public uint flags,time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] private struct MOUSEINPUT { public int x,y; public uint data,flags,time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] private struct INPUTDATA { [FieldOffset(0)] public KEYBDINPUT keyboard; [FieldOffset(0)] public MOUSEINPUT mouse; }
  [StructLayout(LayoutKind.Sequential)] private struct INPUT { public uint type; public INPUTDATA data; }
  [DllImport("user32.dll", SetLastError=true)] private static extern uint SendInput(uint count, INPUT[] input, int size);
  [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
  private static INPUT Key(ushort key, bool up) { return new INPUT { type=1, data=new INPUTDATA { keyboard=new KEYBDINPUT { key=key, flags=up ? 2u : 0u } } }; }
  public static void Clear(IntPtr expected) {
    if(GetForegroundWindow()!=expected) throw new Exception("Target lost focus; text was not cleared.");
    var keys=new INPUT[] { Key(17,false), Key(65,false), Key(65,true), Key(17,true), Key(46,false), Key(46,true) };
    int size=Marshal.SizeOf(typeof(INPUT));
    if(SendInput((uint)keys.Length,keys,size)!=keys.Length) {
      int error=Marshal.GetLastWin32Error();
      var release=new INPUT[] { Key(65,true), Key(17,true), Key(46,true) };
      SendInput((uint)release.Length,release,size);
      throw new Win32Exception(error,"Windows could not clear the selected editor.");
    }
  }
}
'@
"#;
    format!("{helpers}\n{prefix}\n[DeviceBotClearText]::Clear($target)")
}

pub(super) fn window_list_script(process_id: Option<u32>) -> String {
    format!(
        r#"{HELPERS}
Add-Type @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class DeviceBotWindowList {{
  public delegate bool Callback(IntPtr h, IntPtr data);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] private static extern bool EnumWindows(Callback callback, IntPtr data);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr h, StringBuilder text, int count);
  public static long[] Handles() {{
    var result=new List<long>();
    EnumWindows((h,data)=>{{ if(IsWindowVisible(h)) result.Add(h.ToInt64()); return true; }},IntPtr.Zero);
    return result.ToArray();
  }}
  public static string Title(IntPtr h) {{ var text=new StringBuilder(32768); GetWindowText(h,text,text.Capacity); return text.ToString(); }}
}}
'@
$rows = @([DeviceBotWindowList]::Handles() | ForEach-Object {{
  $h = [IntPtr]$_; $owner = [uint32]0
  [void][DeviceBotLive]::GetWindowThreadProcessId($h, [ref]$owner)
  if ($owner -ne 0 -and ({pid} -eq 0 -or $owner -eq {pid})) {{
    $process = Get-Process -Id $owner -ErrorAction SilentlyContinue
    if ($null -ne $process) {{
      [pscustomobject]@{{ processId=$owner; name=$process.ProcessName; windowId=[string]$h.ToInt64(); title=[DeviceBotWindowList]::Title($h) }}
    }}
  }}
}})
ConvertTo-Json -InputObject $rows -Depth 3 -Compress
"#,
        pid = process_id.unwrap_or(0)
    )
}

impl WindowsRobot {
    pub(super) async fn live_geometry_for(&self, target: Target) -> Result<Geometry, RobotError> {
        let output = self
            .powershell(format!(
                "{}\n[DeviceBotLive]::Geometry($target) | ConvertTo-Json -Compress",
                target.script()
            ))
            .await?;
        let values: Vec<i64> =
            serde_json::from_value(parse_powershell_json(&output).map_err(failure)?)
                .map_err(failure)?;
        if values.len() != 4 {
            return Err(failure("invalid target geometry"));
        }
        Ok(Geometry {
            x: values[0].try_into().map_err(failure)?,
            y: values[1].try_into().map_err(failure)?,
            width: values[2].try_into().map_err(failure)?,
            height: values[3].try_into().map_err(failure)?,
        })
    }

    pub(super) async fn live_context(&self, id: &str) -> Result<Value, RobotError> {
        self.require_device(id)?;
        let target = Target::parse(id)?.ok_or_else(|| {
            RobotError::InvalidInput("Select an app and window before opening Windows Live.".into())
        })?;
        let g = self.live_geometry_for(target).await?;
        self.live_geometry.lock().await.insert(id.into(), g);
        Ok(
            json!({"processId":target.pid,"windowId":target.hwnd.to_string(),"originX":g.x,"originY":g.y,"width":g.width & !1,"height":g.height & !1,"captureBackend":"ffmpeg-gdigrab"}),
        )
    }

    pub(super) async fn live_input_prefix(
        &self,
        id: &str,
        points: &[Point],
    ) -> Result<String, RobotError> {
        let Some(target) = Target::parse(id)? else {
            return Ok(String::new());
        };
        let mut script = target.script();
        if let Some(g) = self.live_geometry.lock().await.get(id).copied() {
            script.push_str(&format!("$g = [DeviceBotLive]::Geometry($target)\nif ($g[0] -ne {} -or $g[1] -ne {} -or $g[2] -ne {} -or $g[3] -ne {}) {{ throw 'The selected window moved or resized. Reopen Live before sending input.' }}\n",g.x,g.y,g.width,g.height));
        }
        script.push_str("[DeviceBotLive]::Focus($target)\n");
        for p in points {
            if !p.x.is_finite()
                || !p.y.is_finite()
                || p.x.abs() > i32::MAX as f64
                || p.y.abs() > i32::MAX as f64
            {
                return Err(RobotError::InvalidInput(
                    "Windows input coordinates must be finite physical pixels.".into(),
                ));
            }
            script.push_str(&format!(
                "[DeviceBotLive]::CheckPoint($target, {}, {})\n",
                p.x.round(),
                p.y.round()
            ));
        }
        Ok(script)
    }

    pub(super) async fn live_elements(
        &self,
        id: &str,
        filter: Option<&str>,
        max: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        let target = Target::parse(id)?.ok_or_else(|| failure("missing window target"))?;
        if max == Some(0) {
            return Ok(vec![]);
        }
        // Validate ownership and traverse exactly this HWND, never the first
        // window found for a PID (one process may own several app windows).
        let script=build_uia_script(Some(target.pid),filter,max)
            .replace("$target = $root.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $condition)",
                &format!("$target = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]([int64]{}))\nif ($null -eq $target -or $target.Current.ProcessId -ne {}) {{ throw 'The selected window no longer belongs to the selected app.' }}",target.hwnd,target.pid));
        let output = tokio::time::timeout(UIA_QUERY_TIMEOUT, self.powershell(script))
            .await
            .map_err(failure)??;
        parse_uia_elements(&output)
    }

    pub(super) async fn start_live_video(&self, id: &str) -> Result<VideoStream, RobotError> {
        self.require_device(id)?;
        let target = Target::parse(id)?.ok_or_else(|| {
            RobotError::InvalidInput("Select an app and window before opening Windows Live.".into())
        })?;
        let g = self.live_geometry_for(target).await?;
        if self
            .live_geometry
            .lock()
            .await
            .get(id)
            .is_some_and(|expected| *expected != g)
        {
            return Err(failure(
                "window moved or resized during stream setup; reopen Live",
            ));
        }
        self.live_geometry.lock().await.insert(id.into(), g);
        let ffmpeg = resolve_ffmpeg(
            std::env::var("DEVICE_BOT_FFMPEG_PATH").ok(),
            std::env::current_exe().ok().as_deref(),
        );
        spawn_capture(&ffmpeg, target, g).await
    }
}

fn resolve_ffmpeg(override_path: Option<String>, executable: Option<&std::path::Path>) -> String {
    if let Some(path) = override_path.filter(|path| !path.trim().is_empty()) {
        return path;
    }
    if let Some(directory) = executable.and_then(std::path::Path::parent) {
        for path in [
            directory.join("ffmpeg/bin/ffmpeg.exe"),
            directory.join("ffmpeg.exe"),
        ] {
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    "ffmpeg".into()
}

#[test]
fn resolves_installed_windows_encoder_without_path() {
    let root = std::env::temp_dir().join(format!("windows-live-ffmpeg-{}", std::process::id()));
    let bundled = root.join("ffmpeg/bin/ffmpeg.exe");
    std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
    std::fs::write(&bundled, b"fixture").unwrap();
    assert_eq!(
        resolve_ffmpeg(None, Some(&root.join("device-bot.exe"))),
        bundled.to_string_lossy()
    );
    assert_eq!(
        resolve_ffmpeg(
            Some("custom encoder".into()),
            Some(&root.join("device-bot.exe"))
        ),
        "custom encoder"
    );
    assert_eq!(resolve_ffmpeg(None, None), "ffmpeg");
    std::fs::remove_dir_all(root).unwrap();
}

async fn spawn_capture(
    ffmpeg: &str,
    target: Target,
    g: Geometry,
) -> Result<VideoStream, RobotError> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "gdigrab",
        "-framerate",
        "30",
        "-draw_mouse",
        "1",
        "-i",
    ])
    .arg(format!("hwnd={}", target.hwnd))
    .args([
        "-an",
        "-vf",
        "scale=trunc(iw/2)*2:trunc(ih/2)*2",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-tune",
        "zerolatency",
        "-profile:v",
        "baseline",
        "-pix_fmt",
        "yuv420p",
        "-g",
        "30",
        "-bf",
        "0",
        "-x264-params",
        "aud=1:repeat-headers=1",
        "-f",
        "h264",
        "pipe:1",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    let mut child = cmd.spawn().map_err(|e| {
        failure(format!(
            "FFmpeg 7+ with gdigrab and libx264 required (DEVICE_BOT_FFMPEG_PATH): {e}"
        ))
    })?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let diagnostics = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let mut chunk = [0; 1024];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > 8192 {
                bytes.drain(..bytes.len() - 8192);
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        let result=async {
            let mut buffer=Vec::new(); let mut chunk=[0;16384]; let mut pts=0;
            loop {
                let n=tokio::select! { _=tx.closed()=>return Ok(()), n=stdout.read(&mut chunk)=>n.map_err(failure)? };
                if n==0 { return Err(failure("encoder ended; FFmpeg 7+ with gdigrab hwnd support and libx264 is required")) }
                buffer.extend_from_slice(&chunk[..n]);
                if buffer.len()>16*1024*1024 { return Err(failure("encoded access unit exceeds 16 MiB")) }
                while let Some(data)=take_access_unit(&mut buffer) {
                    let keyframe=data.windows(4).any(|b| b[..3]==[0,0,1] && b[3]&31==5);
                    if tx.send(Ok(VideoFrame{data,pts_us:pts,keyframe})).await.is_err() { return Ok(()) }
                    pts+=33_333;
                }
            }
        }.await;
        let _ = child.kill().await;
        let log = diagnostics.await.unwrap_or_default();
        if let Err(error) = result {
            let _ = tx.try_send(Err(failure(format!("{error}; {log}"))));
        }
    });
    // Fail setup on missing/old FFmpeg or inaccessible desktop instead of
    // returning a successful SDP for a black viewer.
    let first = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .map_err(failure)?
        .ok_or_else(|| failure("capture stopped before its first frame"))??;
    let (ready, frames) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        if ready.send(Ok(first)).await.is_err() {
            return;
        }
        loop {
            let next = tokio::select! { _=ready.closed()=>break, next=rx.recv()=>next };
            let Some(frame) = next else { break };
            if ready.send(frame).await.is_err() {
                break;
            }
        }
    });
    Ok(VideoStream {
        width: g.width & !1,
        height: g.height & !1,
        frames,
    })
}

fn take_access_unit(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut aud = buffer
        .windows(5)
        .enumerate()
        .filter(|(_, b)| b[..4] == [0, 0, 0, 1] && b[4] & 31 == 9)
        .map(|(i, _)| i);
    aud.next()?;
    let end = aud.next()?;
    Some(buffer.drain(..end).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_ids_reject_ambiguous_or_injectable_handles() {
        assert_eq!(
            Target::parse("windows:42:123").unwrap(),
            Some(Target { pid: 42, hwnd: 123 })
        );
        assert!(Target::parse("windows-host").unwrap().is_none());
        for id in [
            "windows:0:1",
            "windows:1:0",
            "windows:1:-1",
            "windows:1:1:2",
            "windows:1:1;evil",
            "windows:4294967296:1",
        ] {
            assert!(Target::parse(id).is_err(), "{id}");
        }
    }
    #[test]
    fn access_units_survive_partial_reads_and_keep_parameter_sets() {
        let mut data = vec![0, 0, 0, 1, 9, 0xf0, 0, 0, 0, 1, 7, 1, 0, 0, 1, 5, 2, 0, 0];
        assert!(take_access_unit(&mut data).is_none());
        data.extend_from_slice(&[0, 1, 9, 0xf0]);
        let unit = take_access_unit(&mut data).unwrap();
        assert!(unit.windows(4).any(|b| b == [0, 0, 1, 5]));
        assert_eq!(data, vec![0, 0, 0, 1, 9, 0xf0]);
    }
    #[tokio::test]
    async fn input_guards_pin_pid_window_geometry_and_hit_test() {
        let robot = WindowsRobot::new("artifacts");
        robot.live_geometry.lock().await.insert(
            "windows:42:123".into(),
            Geometry {
                x: -100,
                y: 20,
                width: 500,
                height: 400,
            },
        );
        let script = robot
            .live_input_prefix("windows:42:123", &[Point { x: 0., y: 30. }])
            .await
            .unwrap();
        assert!(script.contains("[int64]123"));
        assert!(script.contains("[uint32]42"));
        assert!(script.contains("$g[0] -ne -100"));
        assert!(script.contains("CheckPoint($target, 0, 30)"));
        assert!(
            robot
                .live_input_prefix("windows:42:123", &[Point { x: f64::NAN, y: 0. }])
                .await
                .is_err()
        );
    }

    #[test]
    fn window_owner_mismatch_is_rejected_before_capture_or_input() {
        let script = Target {
            pid: 9800,
            hwnd: 5046404,
        }
        .script();
        assert!(script.contains("[DeviceBotLive]::Validate($target, [uint32]9800)"));
        assert!(script.contains("pid!=expected"));
        assert!(
            script.contains("throw new Exception(\"The selected Windows window no longer belongs")
        );
    }

    #[tokio::test]
    async fn clear_compiles_helper_then_validates_window_immediately_before_input() {
        let robot = WindowsRobot::new("artifacts");
        let prefix = robot
            .live_input_prefix("windows:42:123", &[])
            .await
            .unwrap();
        let script = clear_text_script(&prefix);
        let helper = script
            .find("public static class DeviceBotClearText")
            .unwrap();
        let validate = script
            .find("[DeviceBotLive]::Validate($target, [uint32]42)")
            .unwrap();
        let focus = script.find("[DeviceBotLive]::Focus($target)").unwrap();
        let clear = script
            .rfind("[DeviceBotClearText]::Clear($target)")
            .unwrap();
        assert!(helper < validate && validate < focus && focus < clear);
        assert!(script.contains("if(GetForegroundWindow()!=expected) throw"));
        assert!(script.contains(
            "Key(17,false), Key(65,false), Key(65,true), Key(17,true), Key(46,false), Key(46,true)"
        ));
    }

    #[tokio::test]
    async fn session_clones_share_capture_and_input_gate() {
        let first = WindowsRobot::new("artifacts");
        let second = first.clone();
        let gate = first.capture_lock.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), second.capture_lock.lock())
                .await
                .is_err()
        );
        drop(gate);
        let _second = tokio::time::timeout(Duration::from_secs(1), second.capture_lock.lock())
            .await
            .unwrap();
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    async fn fixture(body: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "windows-live-{}-{}",
            std::process::id(),
            SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let executable = dir.join("ffmpeg");
        tokio::fs::write(&executable, format!("#!/bin/sh\n{body}\n"))
            .await
            .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        (dir, executable)
    }

    #[tokio::test]
    async fn capture_fails_setup_with_encoder_diagnostics() {
        let (dir, ffmpeg) = fixture("echo unsupported-hwnd >&2\nexit 1").await;
        let result = spawn_capture(
            ffmpeg.to_str().unwrap(),
            Target { pid: 42, hwnd: 123 },
            Geometry {
                x: 0,
                y: 0,
                width: 501,
                height: 401,
            },
        )
        .await;
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("unsupported-hwnd")
        );
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn capture_pins_handle_and_dropping_receiver_reaps_encoder() {
        let (dir, ffmpeg) = fixture(
            r#"printf '%s\n' "$@" > "$0.args"
echo $$ > "$0.pid"
printf '\000\000\000\001\011\360\000\000\001\145\002\000\000\000\001\011\360'
exec tail -f /dev/null"#,
        )
        .await;
        let stream = spawn_capture(
            ffmpeg.to_str().unwrap(),
            Target { pid: 42, hwnd: 123 },
            Geometry {
                x: -100,
                y: 0,
                width: 501,
                height: 401,
            },
        )
        .await
        .unwrap();
        assert_eq!((stream.width, stream.height), (500, 400));
        let args = tokio::fs::read_to_string(dir.join("ffmpeg.args"))
            .await
            .unwrap();
        assert!(args.contains("-i\nhwnd=123\n"));
        assert!(!args.contains("desktop"));
        assert!(!args.contains("title="));
        let pid = tokio::fs::read_to_string(dir.join("ffmpeg.pid"))
            .await
            .unwrap();
        drop(stream);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let exists = Command::new("kill")
                    .args(["-0", pid.trim()])
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .unwrap()
                    .success();
                if !exists {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("capture child should be killed and reaped when viewer closes");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}

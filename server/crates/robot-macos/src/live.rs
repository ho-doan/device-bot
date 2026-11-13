//! Window-bound ScreenCaptureKit/VideoToolbox live source.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub pid: i32,
    pub window: u32,
}
impl Target {
    pub(super) fn parse(id: &str) -> Result<Option<Self>, RobotError> {
        if id == DEVICE_ID {
            return Ok(None);
        }
        let parts: Vec<_> = id.split(':').collect();
        let invalid = || {
            RobotError::InvalidInput("Expected macos:<positive PID>:<positive window ID>".into())
        };
        if parts.len() != 3
            || parts[0] != "macos"
            || parts[1..]
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(invalid());
        }
        let pid = parts[1].parse::<i32>().map_err(|_| invalid())?;
        let window = parts[2].parse::<u32>().map_err(|_| invalid())?;
        if pid <= 0 || window == 0 {
            return Err(invalid());
        }
        Ok(Some(Self { pid, window }))
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[cfg(target_os = "macos")]
#[repr(C)]
struct Rect {
    origin: CGPoint,
    size: CGSize,
}
#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGRectMakeWithDictionaryRepresentation(dictionary: CFTypeRef, rect: *mut Rect) -> bool;
}

#[cfg(target_os = "macos")]
fn windows() -> Result<Vec<(Target, Geometry)>, RobotError> {
    let list = CfOwned(unsafe {
        CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        )
    });
    if list.0.is_null() {
        return Err(RobotError::CommandFailed(
            "Could not enumerate visible windows".into(),
        ));
    }
    let pid_key = native_cf_string("kCGWindowOwnerPID")?;
    let id_key = native_cf_string("kCGWindowNumber")?;
    let bounds_key = native_cf_string("kCGWindowBounds")?;

    let mut output = Vec::new();
    for i in 0..unsafe { CFArrayGetCount(list.0) } {
        let row = unsafe { CFArrayGetValueAtIndex(list.0, i) };
        if row.is_null() {
            continue;
        }
        let (Some(pid), Some(id)) = (
            native_i32(unsafe { CFDictionaryGetValue(row, pid_key) }),
            native_i32(unsafe { CFDictionaryGetValue(row, id_key) }),
        ) else {
            continue;
        };
        let dict = unsafe { CFDictionaryGetValue(row, bounds_key) };
        let mut rect = Rect {
            origin: CGPoint { x: 0., y: 0. },
            size: CGSize {
                width: 0.,
                height: 0.,
            },
        };
        if dict.is_null() || !unsafe { CGRectMakeWithDictionaryRepresentation(dict, &mut rect) } {
            continue;
        }
        output.push((
            Target {
                pid,
                window: id as u32,
            },
            Geometry {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            },
        ));
    }
    Ok(output)
}
#[cfg(target_os = "macos")]
fn geometry(target: Target) -> Result<Geometry, RobotError> {
    windows()?
        .into_iter()
        .find(|(t, _)| *t == target)
        .map(|(_, g)| g)
        .filter(|g| g.width >= 2. && g.height >= 2.)
        .ok_or_else(|| {
            RobotError::InvalidInput(
                "Selected window is unavailable or belongs to another app; reopen Live".into(),
            )
        })
}
#[cfg(not(target_os = "macos"))]
fn geometry(_: Target) -> Result<Geometry, RobotError> {
    Err(RobotError::Unsupported("macOS Live requires macOS".into()))
}

#[cfg(target_os = "macos")]
fn ax_window(target: Target, g: Geometry) -> Result<CfOwned, RobotError> {
    if !request_accessibility_permission() {
        return Err(RobotError::CommandFailed(
            "Grant Accessibility permission before using macOS Live input or inspection".into(),
        ));
    }
    let attrs = NativeAxAttributes::new()?;
    let app = native_frontmost_application(Some(target.pid))?;
    let list = native_required_attribute(app.0, attrs.windows, "AXWindows")?;
    let mut found = None;
    for index in 0..unsafe { CFArrayGetCount(list.0) } {
        let element = unsafe { CFArrayGetValueAtIndex(list.0, index) };
        let Some(position) = native_point_attribute(element, attrs.position) else {
            continue;
        };
        let Some(size) = native_size_attribute(element, attrs.size) else {
            continue;
        };
        if (position.x - g.x).abs() < 1.
            && (position.y - g.y).abs() < 1.
            && (size.width - g.width).abs() < 1.
            && (size.height - g.height).abs() < 1.
        {
            if found.is_some() {
                return Err(RobotError::InvalidInput(
                    "Selected window has ambiguous accessibility bounds; input was not sent".into(),
                ));
            }
            found = Some(CfOwned(unsafe { CFRetain(element) }));
        }
    }
    found.ok_or_else(|| {
        RobotError::InvalidInput("Cannot match selected window to its accessibility window".into())
    })
}

impl MacosRobot {
    pub(super) fn live_edit_text(&self, target: Target, text: &str, clear: bool) -> bool {
        #[cfg(target_os = "macos")]
        {
            let Ok(app) = native_frontmost_application(Some(target.pid)) else {
                return false;
            };
            let Ok(focused_key) = native_cf_string("AXFocusedUIElement").map(CfOwned) else {
                return false;
            };
            let Some(field) = native_copy_attribute(app.0, focused_key.0) else {
                return false;
            };
            let Ok(key) =
                native_cf_string(if clear { "AXValue" } else { "AXSelectedText" }).map(CfOwned)
            else {
                return false;
            };
            let Ok(value) = native_cf_string(text).map(CfOwned) else {
                return false;
            };
            (unsafe { AXUIElementSetAttributeValue(field.0, key.0, value.0) }) == 0
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (target, text, clear);
            false
        }
    }
    pub(super) async fn live_context(&self, id: &str) -> Result<Value, RobotError> {
        self.require_device(id)?;
        let target = Target::parse(id)?.ok_or_else(|| {
            RobotError::InvalidInput("Select an app/window before opening macOS Live".into())
        })?;
        let g = geometry(target)?;
        prepare_window_capture().await?;
        self.live_geometry.lock().await.insert(id.into(), g);
        Ok(
            json!({"processId":target.pid,"windowId":target.window.to_string(),"originX":g.x,"originY":g.y,"logicalWidth":g.width,"logicalHeight":g.height,"width":(g.width as u32)&!1,"height":(g.height as u32)&!1,"captureBackend":"screencapturekit-videotoolbox"}),
        )
    }
    pub(super) async fn start_live(&self, id: &str) -> Result<robot_core::VideoStream, RobotError> {
        self.require_device(id)?;
        let target = Target::parse(id)?.ok_or_else(|| {
            RobotError::InvalidInput("Select an app/window before opening macOS Live".into())
        })?;
        if !request_screen_recording_permission() {
            return Err(RobotError::CommandFailed(
                "Grant Screen Recording permission, then reopen Live".into(),
            ));
        }
        let g = geometry(target)?;
        if self
            .live_geometry
            .lock()
            .await
            .get(id)
            .is_some_and(|saved| *saved != g)
        {
            return Err(RobotError::InvalidInput(
                "Window moved or resized during negotiation; reopen Live".into(),
            ));
        }
        self.live_geometry.lock().await.insert(id.into(), g);
        start_window_capture(
            target.pid,
            target.window,
            (g.width as u32) & !1,
            (g.height as u32) & !1,
            None,
        )
        .await
    }
    pub(super) async fn live_guard(
        &self,
        id: &str,
        points: &[Point],
    ) -> Result<Target, RobotError> {
        let target = Target::parse(id)?
            .ok_or_else(|| RobotError::InvalidInput("Missing window target".into()))?;
        let g = geometry(target)?;
        let cache = self.live_geometry.lock().await;
        if cache.get(id).is_none_or(|saved| *saved != g) {
            return Err(RobotError::InvalidInput(
                "Window moved/resized or Live is not open; reopen Live before input".into(),
            ));
        }
        drop(cache);
        // The QC viewer can itself own foreground focus. Activate the pinned
        // process, then raise the exact AX window before validating delivery.
        self.apple_script(
            "on run argv\n tell application \"System Events\" to set frontmost of (first process whose unix id is (item 1 of argv as integer)) to true\nend run",
            &[target.pid.to_string()],
        ).await?;
        #[cfg(target_os = "macos")]
        for _ in 0..10 {
            // Activating a process returns before WindowServer publishes its
            // focused application. Wait briefly, then retain the exact window
            // checks below before delivering any input.
            let focused = native_frontmost_application(None).is_ok_and(|front| {
                let mut pid = 0;
                (unsafe { AXUIElementGetPid(front.0, &mut pid) }) == 0 && pid == target.pid
            });
            if focused {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        #[cfg(target_os = "macos")]
        {
            let selected = ax_window(target, g)?;
            let attrs = NativeAxAttributes::new()?;
            let app = native_frontmost_application(Some(target.pid))?;
            let raise = native_cf_string("AXRaise")?;
            let main = native_cf_string("AXMain")?;
            let _ = unsafe { AXUIElementPerformAction(selected.0, raise) };
            let _ = unsafe { AXUIElementSetAttributeValue(selected.0, main, kCFBooleanTrue) };
            let _ =
                unsafe { AXUIElementSetAttributeValue(selected.0, attrs.focused, kCFBooleanTrue) };
            let front = native_frontmost_application(None)?;
            let mut pid = 0;
            if unsafe { AXUIElementGetPid(front.0, &mut pid) } != 0 || pid != target.pid {
                return Err(RobotError::InvalidInput(
                    "Activate the selected app/window before sending input".into(),
                ));
            }
            let focused =
                native_required_attribute(app.0, attrs.focused_window, "AXFocusedWindow")?;
            let Some(p) = native_point_attribute(focused.0, attrs.position) else {
                return Err(RobotError::InvalidInput(
                    "Selected window is not focused".into(),
                ));
            };
            let Some(s) = native_size_attribute(focused.0, attrs.size) else {
                return Err(RobotError::InvalidInput(
                    "Selected window is not focused".into(),
                ));
            };
            if (p.x - g.x).abs() >= 1.
                || (p.y - g.y).abs() >= 1.
                || (s.width - g.width).abs() >= 1.
                || (s.height - g.height).abs() >= 1.
            {
                return Err(RobotError::InvalidInput(
                    "Focus belongs to a different window; input was not sent".into(),
                ));
            }
            drop(selected);
            let listed = windows()?;
            for point in points {
                if !point.x.is_finite()
                    || !point.y.is_finite()
                    || point.x < g.x
                    || point.y < g.y
                    || point.x >= g.x + g.width
                    || point.y >= g.y + g.height
                {
                    return Err(RobotError::InvalidInput(
                        "Input is outside selected window".into(),
                    ));
                }
                let hit = listed.iter().find(|(_, b)| {
                    point.x >= b.x
                        && point.y >= b.y
                        && point.x < b.x + b.width
                        && point.y < b.y + b.height
                });
                if hit.is_none_or(|(t, _)| *t != target) {
                    return Err(RobotError::InvalidInput(
                        "Another window covers the input point".into(),
                    ));
                }
            }
        }
        Ok(target)
    }
    pub(super) async fn live_elements(
        &self,
        id: &str,
        filter: Option<&str>,
        max: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        let target = Target::parse(id)?
            .ok_or_else(|| RobotError::InvalidInput("Missing window target".into()))?;
        let g = geometry(target)?;
        #[cfg(target_os = "macos")]
        {
            let window = ax_window(target, g)?;
            let attrs = NativeAxAttributes::new()?;
            let mut elements = Vec::new();
            native_collect_elements(
                window.0,
                &attrs,
                filter,
                max,
                Instant::now() + NATIVE_FILTER_BUDGET,
                0,
                &mut elements,
            );
            Ok(elements)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (g, filter, max);
            Err(RobotError::Unsupported("macOS Live requires macOS".into()))
        }
    }
}

pub async fn prepare_window_capture() -> Result<PathBuf, RobotError> {
    if let Some(path) = std::env::var_os("DEVICE_BOT_MACOS_LIVE_HELPER").filter(|s| !s.is_empty()) {
        return Ok(path.into());
    }
    use std::hash::{Hash, Hasher};
    let source = include_str!("live_capture.swift");
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    let directory = std::env::temp_dir().join(format!("device-bot-macos-live-{:x}", hash.finish()));
    let binary = directory.join("capture");
    if binary.is_file() {
        return Ok(binary);
    }
    static COMPILE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = COMPILE.lock().await;
    if binary.is_file() {
        return Ok(binary);
    }
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
    let input = directory.join("capture.swift");
    tokio::fs::write(&input, source)
        .await
        .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
    let temporary = directory.join("capture.new");
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        Command::new("xcrun")
            .args(["swiftc", "-swift-version", "5", "-O"])
            .arg(&input)
            .arg("-module-cache-path")
            .arg(directory.join("modules"))
            .arg("-o")
            .arg(&temporary)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| RobotError::CommandFailed("macOS Live helper compilation timed out".into()))?
    .map_err(|e| {
        RobotError::CommandFailed(format!(
            "macOS Live needs Xcode Command Line Tools or DEVICE_BOT_MACOS_LIVE_HELPER: {e}"
        ))
    })?;
    if !output.status.success() {
        return Err(RobotError::CommandFailed(format!(
            "Could not compile macOS Live helper: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    tokio::fs::rename(temporary, &binary)
        .await
        .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
    Ok(binary)
}

/// Start a native, window-scoped capture. Crop coordinates are logical points
/// relative to the captured window; the simulator adapter resolves its content.
pub async fn start_window_capture(
    pid: i32,
    window: u32,
    width: u32,
    height: u32,
    source_rect: Option<[f64; 4]>,
) -> Result<robot_core::VideoStream, RobotError> {
    if pid <= 0 || window == 0 || width < 2 || height < 2 {
        return Err(RobotError::InvalidInput(
            "Invalid native capture target or dimensions".into(),
        ));
    }
    let helper = prepare_window_capture().await?;
    let mut command = Command::new(helper);
    command.args([
        pid.to_string(),
        window.to_string(),
        width.to_string(),
        height.to_string(),
    ]);
    if let Some(rect) = source_rect {
        if rect.iter().any(|v| !v.is_finite())
            || rect[0] < 0.
            || rect[1] < 0.
            || rect[2] < 2.
            || rect[3] < 2.
        {
            return Err(RobotError::InvalidInput(
                "Invalid native capture source rectangle".into(),
            ));
        }
        command.args(rect.map(|v| v.to_string()));
    }
    robot_core::video::start_h264_process(command, width, height).await
}
#[cfg(target_os = "macos")]
pub(super) fn scroll(at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
    if !dx.is_finite() || !dy.is_finite() || dx.abs() > 10000. || dy.abs() > 10000. {
        return Err(RobotError::InvalidInput("Invalid scroll delta".into()));
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateScrollWheelEvent2(
            source: CFTypeRef,
            units: u32,
            wheels: u32,
            vertical: i32,
            horizontal: i32,
            third: i32,
        ) -> CFTypeRef;
        fn CGEventSetLocation(event: CFTypeRef, point: CGPoint);
        fn CGEventPost(tap: u32, event: CFTypeRef);
    }
    let event = CfOwned(unsafe {
        CGEventCreateScrollWheelEvent2(
            std::ptr::null(),
            0,
            2,
            -dy.round() as i32,
            -dx.round() as i32,
            0,
        )
    });
    if event.0.is_null() {
        return Err(RobotError::CommandFailed(
            "Could not create native scroll event".into(),
        ));
    }
    unsafe {
        CGEventSetLocation(event.0, CGPoint { x: at.x, y: at.y });
        CGEventPost(0, event.0)
    };
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub(super) fn scroll(_: Point, _: f64, _: f64) -> Result<(), RobotError> {
    Err(RobotError::Unsupported("macOS Live requires macOS".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_ids_require_unambiguous_positive_numeric_identity() {
        assert_eq!(
            Target::parse("macos:123:456").unwrap(),
            Some(Target {
                pid: 123,
                window: 456
            })
        );
        assert_eq!(Target::parse("macos-host").unwrap(), None);
        for id in [
            "macos:0:4",
            "macos:1:0",
            "macos:-1:3",
            "macos:1:2:3",
            "macos:1:2;exit",
            "macos:2147483648:1",
        ] {
            assert!(Target::parse(id).is_err(), "{id}")
        }
    }

    // Run only against a disposable animated fixture window, never a customer app.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "requires DEVICE_BOT_MACOS_LIVE_TEST_PID/WINDOW and Screen Recording"]
    async fn real_window_stream_rejects_wrong_owner_and_stale_geometry() {
        let pid = std::env::var("DEVICE_BOT_MACOS_LIVE_TEST_PID")
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let window = std::env::var("DEVICE_BOT_MACOS_LIVE_TEST_WINDOW")
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let robot = MacosRobot::new(std::env::temp_dir());
        let id = format!("macos:{pid}:{window}");
        let context = robot.stream_context(&id).await.unwrap();
        assert!(
            robot
                .stream_context(&format!("macos:{}:{window}", pid + 1))
                .await
                .is_err()
        );
        let mut stream = robot.start_video_stream(&id).await.unwrap();
        assert_eq!(u64::from(stream.width), context["width"].as_u64().unwrap());
        assert_eq!(
            u64::from(stream.height),
            context["height"].as_u64().unwrap()
        );
        let frame = tokio::time::timeout(std::time::Duration::from_secs(10), stream.frames.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(frame.keyframe);
        assert!(frame.data.len() > 100);
        drop(stream);
        let mut cache = robot.live_geometry.lock().await;
        cache.get_mut(&id).unwrap().width += 2.;
        drop(cache);
        let error = robot.live_guard(&id, &[]).await.unwrap_err().to_string();
        assert!(error.contains("moved/resized"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "requires disposable Device Bot Live Fixture and Accessibility"]
    async fn real_window_input_reaches_only_disposable_fixture() {
        let pid = std::env::var("DEVICE_BOT_MACOS_LIVE_TEST_PID")
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let window = std::env::var("DEVICE_BOT_MACOS_LIVE_TEST_WINDOW")
            .unwrap()
            .parse::<u32>()
            .unwrap();
        assert!(
            native_listed_windows()
                .unwrap()
                .iter()
                .any(|w| w.process_id == pid as u32
                    && w.window_id == window.to_string()
                    && w.title == "Device Bot Live Fixture")
        );
        let robot = MacosRobot::new(std::env::temp_dir());
        let id = format!("macos:{pid}:{window}");
        robot.stream_context(&id).await.unwrap();
        let elements = robot.list_elements(&id).await.unwrap();
        let editor = elements
            .iter()
            .find(|e| e.class_name.contains("TextField"))
            .expect("fixture editor");
        let at = Point {
            x: (editor.bounds.left + editor.bounds.right) as f64 / 2.,
            y: (editor.bounds.top + editor.bounds.bottom) as f64 / 2.,
        };
        robot.tap(&id, at).await.unwrap();
        robot.clear_text(&id).await.unwrap();
        robot
            .type_text(&id, "Live fixture verified — tiếng Việt 🦀")
            .await
            .unwrap();
        let elements = robot.list_elements(&id).await.unwrap();
        assert!(
            elements.iter().any(|e| e.value.as_deref()
                == Some("Live fixture verified — tiếng Việt 🦀")
                || e.text.as_deref() == Some("Live fixture verified — tiếng Việt 🦀")),
            "{elements:?}"
        );
        robot.clear_text(&id).await.unwrap();
        let elements = robot.list_elements(&id).await.unwrap();
        assert!(
            elements
                .iter()
                .any(|e| e.class_name.contains("TextField") && e.value.as_deref() == Some(""))
        );
    }
}

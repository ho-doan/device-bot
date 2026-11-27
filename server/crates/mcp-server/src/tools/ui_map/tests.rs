use std::{path::PathBuf, sync::Mutex};

use async_trait::async_trait;
use robot_core::{
    Bounds, ElementType, Platform, Point, RobotDevice, RobotError, UiElement, UiRunnerStatus,
};

use super::*;

async fn scan(
    robot: &Arc<dyn Robot>,
    opt: &Options,
    stop: &AtomicBool,
    map: &mut UiMap,
    dir: &Path,
) -> Result<(), String> {
    crawl::scan(robot, opt, stop, map, dir, &timing::TimingLog::new()).await
}

#[tokio::test(start_paused = true)]
async fn timings_include_a_phase_cancelled_by_the_scan_deadline() {
    let dir = Temp::new();
    let timing = timing::TimingLog::new();
    let result = tokio::time::timeout(
        Duration::from_millis(125),
        timing.measure("ui_tree", std::future::pending::<()>()),
    )
    .await;
    assert!(result.is_err());
    timing.save(&dir.0).await.unwrap();
    let report: Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("timing.json")).unwrap()).unwrap();
    assert_eq!(
        report["phases"]["ui_tree"],
        json!({"calls":1,"totalMs":125,"maxMs":125})
    );
}

#[derive(Default)]
struct Fixture(Mutex<FixtureState>);

#[derive(Default)]
struct FixtureState {
    page: usize,
    macos_flutter: bool,
    viewport_fixture: bool,
    viewport_last_slice: Option<i32>,
    android_discovery_stride: Option<i32>,
    android_replay_stalls: bool,
    scroll_count_at_below_tap: Option<usize>,
    no_scroll_restore: bool,
    viewport_slide_reads: Option<usize>,
    viewport_content_shift_on_delete: Option<bool>,
    slice: i32,
    scrolls: Vec<i32>,
    native_window_reads: usize,
    flutter_reads: usize,
    android: bool,
    ios: bool,
    left_no_longer_reproducible: bool,
    delay_android_foreground_on_launch: bool,
    foreground_reads_until_ready: u8,
    disconnect_on_tap: Option<&'static str>,
    extra_left_action: bool,
    disconnected: bool,
    recoveries: usize,
    xctest_generation: Option<usize>,
    xctest_uuid: bool,
    tree_delay_ms: u64,
    focused_tree_delay_ms: u64,
    duplicate_editor_after_focus: bool,
    tap_delay_ms: u64,
    focus: bool,
    entered: bool,
    checked: bool,
    show_extra_control_when_checked: bool,
    empty_queries: usize,
    taps: Vec<String>,
    resets: usize,
}

impl Fixture {
    fn assert_current_macos_device(state: &FixtureState, id: &str) {
        if state.macos_flutter {
            assert_eq!(
                id,
                if state.resets == 0 {
                    "macos:41:42"
                } else {
                    "macos:51:52"
                }
            );
        }
    }

    fn reset(&self) {
        let mut state = self.0.lock().unwrap();
        state.page = 0;
        state.entered = false;
        state.checked = false;
        state.focus = false;
        state.slice = 0;
        if let Some(shifted) = &mut state.viewport_content_shift_on_delete {
            *shifted = false;
        }
        if state.delay_android_foreground_on_launch {
            state.foreground_reads_until_ready = 2;
        }
        state.resets += 1;
    }
}

fn node(label: &str, id: &str, x: i32, kind: ElementType) -> UiElement {
    UiElement {
        class_name: "fixture".into(),
        resource_id: Some(id.into()),
        text: Some(label.into()),
        content_description: Some(label.into()),
        bounds: Bounds {
            left: x - 5,
            right: x + 5,
            top: 0,
            bottom: 20,
        },
        clickable: kind != ElementType::StaticText,
        enabled: true,
        focused: Some(false),
        checked: None,
        selected: None,
        value: None,
        element_type: Some(kind),
        web_context: None,
    }
}

#[async_trait]
impl Robot for Fixture {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let state = self.0.lock().unwrap();
        let (id, platform) = if state.macos_flutter {
            ("macos-host", Platform::Macos)
        } else if state.android {
            (
                if state.recoveries == 0 {
                    "android-fixture"
                } else {
                    "android-recovered"
                },
                Platform::Android,
            )
        } else if state.ios {
            ("ios-fixture", Platform::Ios)
        } else {
            ("chrome:fixture", Platform::Chrome)
        };
        Ok(vec![RobotDevice {
            id: id.into(),
            name: "Fixture".into(),
            platform,
            state: if state.android && state.disconnected {
                robot_core::DeviceState::Offline
            } else {
                robot_core::DeviceState::Available
            },
        }])
    }

    async fn recovery_boot_target(&self, id: &str) -> Result<Option<String>, RobotError> {
        let state = self.0.lock().unwrap();
        Ok(
            (state.android && matches!(id, "android-fixture" | "android-recovered"))
                .then(|| "avd:test".into()),
        )
    }

    async fn boot_device(&self, id: &str) -> Result<(), RobotError> {
        assert_eq!(id, "avd:test");
        let mut state = self.0.lock().unwrap();
        assert!(state.android && state.disconnected);
        state.disconnected = false;
        state.recoveries += 1;
        state.foreground_reads_until_ready = 1;
        Ok(())
    }

    async fn stream_context(&self, _: &str) -> Result<Value, RobotError> {
        let mut state = self.0.lock().unwrap();
        assert!(!state.ios, "iOS scans must not depend on the Live window");
        if state.android {
            if state.disconnected {
                return Err(RobotError::CommandFailed(
                    "adb exited: device offline".into(),
                ));
            }
            if state.foreground_reads_until_ready > 0 {
                state.foreground_reads_until_ready -= 1;
                return Ok(json!({"appId":"launcher.app"}));
            }
            return Ok(json!({"appId": if state.page == 3 { "other.app" } else { "fixture.app" }}));
        }
        let url = [
            "http://localhost/",
            "http://localhost/left",
            "http://localhost/right",
            "http://outside.example/",
        ][state.page];
        Ok(json!({"url": url}))
    }

    async fn ui_runner_status(&self, _: &str) -> Result<UiRunnerStatus, RobotError> {
        let state = self.0.lock().unwrap();
        assert!(state.ios);
        Ok(UiRunnerStatus {
            ready: state.page != 3 && !state.disconnected,
            state: if state.disconnected {
                "disconnected".into()
            } else {
                "ready".into()
            },
            detail: if state.disconnected {
                "iOS XCTest agent disconnected".into()
            } else {
                String::new()
            },
            target_app_id: Some("fixture.app".into()),
            ..Default::default()
        })
    }

    async fn reboot_helper(
        &self,
        id: &str,
        app: Option<&str>,
    ) -> Result<UiRunnerStatus, RobotError> {
        assert_eq!(id, "ios-fixture");
        assert_eq!(app, Some("fixture.app"));
        {
            let mut state = self.0.lock().unwrap();
            assert!(state.ios && state.disconnected);
            state.disconnected = false;
            state.recoveries += 1;
        }
        self.reset();
        self.ui_runner_status(id).await
    }

    async fn take_screenshot(&self, id: &str) -> Result<Vec<u8>, RobotError> {
        Self::assert_current_macos_device(&self.0.lock().unwrap(), id);
        Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec())
    }

    async fn tap(&self, id: &str, point: Point) -> Result<(), RobotError> {
        let delay = self.0.lock().unwrap().tap_delay_ms;
        tokio::time::sleep(Duration::from_millis(delay)).await;
        let mut state = self.0.lock().unwrap();
        Self::assert_current_macos_device(&state, id);
        let x = if state.macos_flutter {
            assert!(point.x < 0., "the fixture is left of the primary display");
            point.x as i32 + 1000
        } else {
            point.x as i32
        };
        let action = match (state.page, x) {
            (0, 10) => {
                if !state.left_no_longer_reproducible || !state.taps.iter().any(|tap| tap == "left")
                {
                    state.page = 1;
                }
                "left"
            }
            (0, 20) => {
                state.page = 2;
                "right"
            }
            (0, 30) => {
                state.focus = true;
                "editor"
            }
            (0, 40) => {
                if let Some(shifted) = &mut state.viewport_content_shift_on_delete {
                    *shifted = true;
                }
                "delete"
            }
            (0, 50) => {
                state.page = 3;
                "external"
            }
            (0, 70) => {
                state.checked = !state.checked;
                "toggle"
            }
            (0, 80)
                if state.viewport_fixture
                    && state.slice == state.viewport_last_slice.unwrap_or(1) =>
            {
                state.scroll_count_at_below_tap = Some(state.scrolls.len());
                state.page = 1;
                state.slice = 0;
                "below"
            }
            (1 | 2, 10) => {
                state.page = 0;
                "back"
            }
            (1, 20) => "same",
            (1, 30) if state.extra_left_action => "after",
            _ => return Err(RobotError::CommandFailed("Unexpected tap".into())),
        };
        state.focus = action == "editor";
        state.taps.push(action.into());
        if state.disconnect_on_tap == Some(action) {
            state.disconnect_on_tap = None;
            state.disconnected = true;
            let detail = if state.android {
                "adb exited: device offline"
            } else {
                "iOS XCTest agent disconnected"
            };
            return Err(RobotError::CommandFailed(detail.into()));
        }
        Ok(())
    }

    async fn launch_app(&self, _: &str, app: &str) -> Result<(), RobotError> {
        assert_eq!(app, "fixture.app");
        self.reset();
        Ok(())
    }

    async fn terminate_app(&self, _: &str, app: &str) -> Result<(), RobotError> {
        assert_eq!(app, "fixture.app");
        Ok(())
    }

    async fn bind_window_fixture(&self, id: &str, app: &str) -> Result<(), RobotError> {
        Self::assert_current_macos_device(&self.0.lock().unwrap(), id);
        if app != "fixture.app" {
            return Err(RobotError::InvalidInput(
                "Fixture bundle does not match".into(),
            ));
        }
        Ok(())
    }

    async fn restart_window_fixture(&self, id: &str) -> Result<String, RobotError> {
        Self::assert_current_macos_device(&self.0.lock().unwrap(), id);
        self.reset();
        Ok("macos:51:52".into())
    }

    async fn open_url(&self, _: &str, url: &str, _: Option<&str>) -> Result<(), RobotError> {
        assert_eq!(url, "http://localhost/");
        self.reset();
        Ok(())
    }

    async fn clear_text(&self, _: &str) -> Result<(), RobotError> {
        assert!(self.0.lock().unwrap().focus);
        Ok(())
    }

    async fn type_text(&self, _: &str, text: &str) -> Result<(), RobotError> {
        assert_eq!(text, "synthetic-QC-value");
        let mut s = self.0.lock().unwrap();
        assert!(s.focus);
        s.entered = true;
        Ok(())
    }

    async fn scroll(&self, id: &str, at: Point, dx: f64, dy: f64) -> Result<(), RobotError> {
        let mut state = self.0.lock().unwrap();
        assert!(state.viewport_fixture);
        Self::assert_current_macos_device(&state, id);
        assert_eq!(
            (at.x, at.y, dx),
            (if state.macos_flutter { -850. } else { 150. }, 100., 0.)
        );
        if state.android || state.ios {
            assert!((0.0..=200.0).contains(&(at.y - dy)));
        }
        state.scrolls.push(dy as i32);
        if state.page == 0 && !(state.no_scroll_restore && dy < 0.) {
            state.slice =
                (state.slice + dy.signum() as i32).clamp(0, state.viewport_last_slice.unwrap_or(1));
        }
        Ok(())
    }

    async fn swipe(
        &self,
        id: &str,
        from: Point,
        to: Point,
        duration: Option<u64>,
    ) -> Result<(), RobotError> {
        let mut state = self.0.lock().unwrap();
        assert!(state.android && state.viewport_fixture);
        assert_eq!(id, "android-fixture");
        assert_eq!((from.x, to.x, duration), (150., 150., Some(420)));
        assert!((from.y == 160. && to.y == 80.) || (from.y == 80. && to.y == 160.));
        let delta = (from.y - to.y) as i32;
        let replaying = delta > 0 && state.scrolls.iter().any(|&previous| previous < 0);
        state.scrolls.push(delta);
        if state.page == 0 && !(replaying && state.android_replay_stalls) {
            let stride = if replaying {
                1
            } else {
                state.android_discovery_stride.unwrap_or(1)
            };
            state.slice = (state.slice + delta.signum() * stride)
                .clamp(0, state.viewport_last_slice.unwrap_or(1));
        }
        Ok(())
    }
    async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        unreachable!()
    }
    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        unreachable!()
    }

    async fn list_flutter_elements(&self, device: &str) -> Result<Vec<UiElement>, RobotError> {
        let mut visible = false;
        if device.starts_with("macos:") {
            let mut state = self.0.lock().unwrap();
            if state.macos_flutter {
                Self::assert_current_macos_device(&state, device);
                state.flutter_reads += 1;
                // The bridge briefly returns no tree while the window settles.
                visible = state.flutter_reads != 2;
            }
        }
        if visible {
            self.list_elements("fixture-flutter").await
        } else {
            Ok(vec![])
        }
    }

    async fn list_elements(&self, device: &str) -> Result<Vec<UiElement>, RobotError> {
        let delay = {
            let state = self.0.lock().unwrap();
            if state.focus && state.focused_tree_delay_ms != 0 {
                state.focused_tree_delay_ms
            } else {
                state.tree_delay_ms
            }
        };
        if delay != 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let mut s = self.0.lock().unwrap();
        if s.macos_flutter && device == "macos:41:42" {
            s.native_window_reads += 1;
            let mut chrome = node("", "", 10, ElementType::Button);
            chrome.resource_id = None;
            return Ok(vec![chrome]);
        }
        if s.empty_queries > 0 {
            s.empty_queries -= 1;
            return Ok(Vec::new());
        }
        let xctest_generation = s.xctest_generation.as_mut().map(|generation| {
            *generation += 1;
            *generation
        });
        let mut rows = match s.page {
            0 => {
                let mut editor = node("Search", "search", 30, ElementType::TextField);
                editor.focused = Some(s.focus);
                editor.value = s.entered.then(|| "synthetic-QC-value".into());
                let mut toggle = node("Toggle task", "toggle", 70, ElementType::CheckBox);
                toggle.checked = Some(s.checked);
                let mut rows = vec![
                    node("Left", "left", 10, ElementType::Button),
                    node("Right", "right", 20, ElementType::Button),
                    editor,
                    node("Delete data", "delete", 40, ElementType::Button),
                    node("External help", "external", 50, ElementType::Link),
                    toggle,
                ];
                if s.focus && s.duplicate_editor_after_focus {
                    rows.push(node("Search", "search", 100, ElementType::TextField));
                }
                if let Some(generation) = xctest_generation {
                    let snapshot_id = |slot| {
                        if s.xctest_uuid {
                            format!("ios-{}", Uuid::from_u128((generation as u128) * 2 + slot))
                        } else {
                            format!("ios-{generation}-{slot}")
                        }
                    };
                    rows[0].resource_id = Some(snapshot_id(0));
                    let mut banner = node("Banner", &snapshot_id(1), 90, ElementType::StaticText);
                    banner.clickable = true;
                    rows.push(banner);
                }
                if s.checked && s.show_extra_control_when_checked {
                    rows.push(node("Extra action", "extra", 80, ElementType::Button));
                }
                if s.entered {
                    rows.push(node(
                        "Search results",
                        "results",
                        60,
                        ElementType::StaticText,
                    ));
                }
                rows
            }
            1 => {
                let mut controls = vec![
                    node("Back", "back", 10, ElementType::Button),
                    node("Same state", "same", 20, ElementType::Button),
                ];
                if s.extra_left_action {
                    controls.push(node("After recovery", "after", 30, ElementType::Button));
                }
                controls
            }
            2 => vec![node("Back", "back", 10, ElementType::Button)],
            _ => panic!("must stop before inspecting outside origin"),
        };
        if (s.macos_flutter || s.android || s.ios) && s.viewport_fixture {
            let mut viewport = node("", "fixture-viewport", 150, ElementType::StaticText);
            viewport.class_name = if s.android {
                "android.widget.ScrollView".into()
            } else if s.ios {
                viewport.resource_id = Some(format!("ios-{}-5", s.xctest_generation.unwrap_or(0)));
                "scrollView".into()
            } else {
                "scrollView Scrollable".into()
            };
            viewport.element_type = None;
            let left = if s.page == 1 {
                s.viewport_slide_reads.as_mut().map_or(0, |reads| {
                    let left = [80, 40, 20, 10, 0].get(*reads).copied().unwrap_or(0);
                    *reads += 1;
                    left
                })
            } else {
                0
            };
            viewport.bounds = Bounds {
                left,
                top: 0,
                right: 300,
                bottom: 200,
            };
            rows.push(viewport);
            if s.page == 0 && s.viewport_last_slice.is_some() {
                rows.push(node(
                    &format!("Position {}", s.slice),
                    "slice-marker",
                    120,
                    ElementType::StaticText,
                ));
            }
            if s.page == 0 && s.slice == s.viewport_last_slice.unwrap_or(1) {
                rows.retain(|row| row.resource_id.as_deref() != Some("left"));
                rows.push(node("Below fold", "below", 80, ElementType::Button));
            }
            if s.viewport_content_shift_on_delete == Some(true) {
                for row in rows.iter_mut().filter(|row| row.clickable) {
                    row.bounds.top += 30;
                    row.bounds.bottom += 30;
                }
            }
        }
        if s.macos_flutter {
            for row in &mut rows {
                row.bounds.left -= 1000;
                row.bounds.right -= 1000;
            }
        }
        Ok(rows)
    }
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("device-bot-map-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn graph() -> UiMap {
    UiMap {
        id: Uuid::new_v4().to_string(),
        device_id: "chrome:fixture".into(),
        status: "running".into(),
        started_at_ms: 0,
        attempted: 0,
        screens: vec![],
        transitions: vec![],
        source_routes: vec![],
        notes: vec![],
    }
}

#[tokio::test(start_paused = true)]
async fn macos_viewport_reveals_controls_and_exports_scroll_before_tap() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        viewport_fixture: true,
        viewport_slide_reads: Some(0),
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "allowedTaps": ["below"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    let root = &map.screens[0];
    assert_eq!(root.native_viewport.as_ref().unwrap().revealed_controls, 1);
    let target = root
        .controls
        .iter()
        .find(|row| row.label == "Below fold")
        .unwrap();
    assert_eq!(target.initially_offscreen, Some(true));
    assert_eq!(target.status, "observed", "{:?}", target.note);
    let evidence = target.evidence.as_ref().unwrap();
    let yaml = std::fs::read_to_string(dir.0.join(&evidence.yaml)).unwrap();
    assert!(yaml.find("scroll:").unwrap() < yaml.find("tap_element:").unwrap());
    assert!(yaml.contains("wheel_delta: 140"));
    assert!(yaml.contains("resource_id: fixture-viewport"));
    assert!(yaml.contains("stable_samples: 2"));
    assert_eq!(map.transitions.len(), 1);
    assert_eq!(map.transitions[0].label, "Below fold");
    assert_eq!(
        map.screens[1].native_viewport.as_ref().unwrap().bounds.left,
        -1000
    );
    assert_eq!(fixture.0.lock().unwrap().taps, ["below"]);
}

#[tokio::test(start_paused = true)]
async fn mobile_native_viewports_reveal_and_tap_controls_outside_entry_tree() {
    for (device_id, class_name, last_slice) in [
        ("ios-fixture", "scrollView", None),
        ("android-fixture", "android.widget.ScrollView", Some(6)),
    ] {
        let dir = Temp::new();
        let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
            ios: device_id == "ios-fixture",
            android: device_id == "android-fixture",
            xctest_generation: (device_id == "ios-fixture").then_some(0),
            viewport_fixture: true,
            viewport_last_slice: last_slice,
            ..Default::default()
        })));
        let robot: Arc<dyn Robot> = fixture.clone();
        let mut map = graph();
        let opt: Options = serde_json::from_value(json!({
            "deviceId": device_id, "allowedTaps": ["below"]
        }))
        .unwrap();
        scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
            .await
            .unwrap();
        let target = map.screens[0]
            .controls
            .iter()
            .find(|row| row.label == "Below fold")
            .unwrap();
        assert_eq!(target.initially_offscreen, Some(true));
        assert_eq!(target.status, "observed", "{:?}", target.note);
        let yaml =
            std::fs::read_to_string(dir.0.join(&target.evidence.as_ref().unwrap().yaml)).unwrap();
        let reveal = if device_id == "android-fixture" {
            "gesture:"
        } else {
            "scroll:"
        };
        assert!(yaml.find(reveal).unwrap() < yaml.find("tap_element:").unwrap());
        assert!(yaml.contains(class_name));
        assert_eq!(
            yaml.matches(reveal).count(),
            last_slice.unwrap_or(1) as usize
        );
        assert!(!map.screens[0].native_viewport.as_ref().unwrap().hit_limit);
        assert_eq!(fixture.0.lock().unwrap().taps, ["below"]);
    }
}

#[tokio::test(start_paused = true)]
async fn android_reveal_uses_live_progress_instead_of_the_discovery_swipe_count() {
    for stalls in [false, true] {
        let dir = Temp::new();
        let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
            android: true,
            viewport_fixture: true,
            viewport_last_slice: Some(6),
            android_discovery_stride: Some(2),
            android_replay_stalls: stalls,
            ..Default::default()
        })));
        let robot: Arc<dyn Robot> = fixture.clone();
        let mut map = graph();
        let opt: Options = serde_json::from_value(json!({
            "deviceId": "android-fixture", "allowedTaps": ["below"]
        }))
        .unwrap();
        scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
            .await
            .unwrap();

        let target = map.screens[0]
            .controls
            .iter()
            .find(|row| row.resource_id.as_deref() == Some("below"))
            .unwrap();
        let state = fixture.0.lock().unwrap();
        let scrolls = &state.scrolls[..state
            .scroll_count_at_below_tap
            .unwrap_or(state.scrolls.len())];
        let forward = scrolls.iter().rev().take_while(|&&dy| dy > 0).count();
        if stalls {
            assert_eq!(forward, 1);
            assert!(state.taps.is_empty());
            assert_eq!(target.status, "failed");
            assert!(
                target
                    .note
                    .as_deref()
                    .unwrap()
                    .contains("stopped progressing")
            );
        } else {
            assert_eq!(forward, 6);
            assert_eq!(state.taps, ["below"]);
            assert_eq!(target.status, "observed", "{:?}", target.note);
            let yaml = std::fs::read_to_string(dir.0.join(&target.evidence.as_ref().unwrap().yaml))
                .unwrap();
            assert_eq!(yaml.matches("gesture:").count(), forward);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn scoped_mobile_relaunches_to_verify_a_control_after_a_probe_cannot_rewind() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        viewport_fixture: true,
        no_scroll_restore: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "resetAppId":"fixture.app", "allowedTaps":["below"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert!(
        map.notes
            .iter()
            .any(|note| note.contains("Could not restore the original viewport"))
    );
    assert_eq!(fixture.0.lock().unwrap().taps, ["below"]);
    assert_eq!(map.transitions.len(), 1);
}

#[test]
fn native_viewport_recognizes_ax_scroll_area_but_not_two_independent_panes() {
    let mut left = node("", "", -850, ElementType::StaticText);
    left.resource_id = None;
    left.class_name = "AXScrollArea".into();
    left.bounds = Bounds {
        left: -1000,
        top: 0,
        right: -700,
        bottom: 200,
    };
    let found = viewport::inspect(&[left.clone()], false).unwrap().unwrap();
    assert_eq!(found.center(), Point { x: -850., y: 100. });
    let mut right = left.clone();
    right.bounds.left = -600;
    right.bounds.right = -300;
    assert!(viewport::inspect(&[left, right], false).is_err());
}

#[test]
fn recorded_scroll_anchor_survives_a_window_move_but_not_a_different_slice() {
    let mut viewport_node = node("", "", 0, ElementType::StaticText);
    viewport_node.class_name = "AXScrollArea".into();
    viewport_node.bounds = Bounds {
        left: -1000,
        top: -20,
        right: -700,
        bottom: 180,
    };
    let button = node("Button", "stable-anchor", -850, ElementType::Button);
    let original = viewport::inspect(&[viewport_node.clone(), button.clone()], false)
        .unwrap()
        .unwrap();
    let move_window = |mut element: UiElement| {
        element.bounds.left += 1100;
        element.bounds.right += 1100;
        element.bounds.top += 50;
        element.bounds.bottom += 50;
        element
    };
    let moved_port = move_window(viewport_node);
    let mut moved_button = move_window(button);
    let moved = viewport::inspect(&[moved_port.clone(), moved_button.clone()], false)
        .unwrap()
        .unwrap();
    assert!(original.same_replay_anchor(&moved));
    assert!(!original.same_position(&moved));
    moved_button.bounds.top += 30;
    moved_button.bounds.bottom += 30;
    let other_slice = viewport::inspect(&[moved_port, moved_button], false)
        .unwrap()
        .unwrap();
    assert!(!original.same_replay_anchor(&other_slice));
}

#[tokio::test(start_paused = true)]
async fn macos_fixture_recovers_offscreen_control_after_same_screen_layout_change() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        viewport_fixture: true,
        viewport_content_shift_on_delete: Some(false),
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "resetAppId": "fixture.app",
        "allowedTaps": ["delete", "below"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    let entry = &map.screens[0].controls;
    assert_eq!(
        entry
            .iter()
            .find(|c| c.label == "Delete data")
            .unwrap()
            .status,
        "no_change"
    );
    let hidden = entry.iter().find(|c| c.label == "Below fold").unwrap();
    assert_eq!(hidden.status, "observed", "{:?}", hidden.note);
    assert_eq!(hidden.initially_offscreen, Some(true));
    assert_eq!(map.device_id, "macos:51:52");
    assert_eq!(fixture.0.lock().unwrap().taps, ["delete", "below"]);
}

#[tokio::test(start_paused = true)]
async fn macos_viewport_does_not_tap_if_the_entry_slice_cannot_be_restored() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        viewport_fixture: true,
        no_scroll_restore: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "allowedTaps": ["left", "below"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert!(
        map.notes
            .iter()
            .any(|note| note.contains("Could not restore the original viewport"))
    );
    assert!(fixture.0.lock().unwrap().taps.is_empty());
    assert!(map.transitions.is_empty());
}

#[tokio::test(start_paused = true)]
async fn macos_fixture_relaunches_and_uses_the_new_window_to_try_a_sibling() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "resetAppId":"fixture.app",
        "allowedTaps": ["left", "right"], "backControl": "not available"
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(
        map.transitions
            .iter()
            .map(|edge| edge.label.as_str())
            .collect::<Vec<_>>(),
        ["Left", "Right"]
    );
    assert_eq!(map.device_id, "macos:51:52");
    assert_eq!(fixture.0.lock().unwrap().resets, 1);
}

#[tokio::test(start_paused = true)]
async fn macos_fixture_with_mismatched_bundle_stops_before_input() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "resetAppId": "other.app", "allButtons": true
    }))
    .unwrap();
    let error = scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap_err();
    assert!(error.contains("Fixture bundle does not match"));
    assert!(map.screens.is_empty());
    let state = fixture.0.lock().unwrap();
    assert_eq!(state.resets, 0);
    assert!(state.taps.is_empty());
}

#[tokio::test(start_paused = true)]
async fn macos_flutter_map_replays_against_the_exact_window() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        macos_flutter: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "macos:41:42", "allowedTaps": ["left"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.transitions.len(), 1, "{:?}", map.notes);
    let evidence = map.screens[0].controls[0].evidence.as_ref().unwrap();
    let yaml = std::fs::read_to_string(dir.0.join(&evidence.yaml)).unwrap();
    assert!(yaml.contains("source: flutter"));
    assert!(evidence.screenshot.is_some());
    let state = fixture.0.lock().unwrap();
    assert_eq!(state.taps, ["left"]);
    assert_eq!(state.native_window_reads, 1);
}

#[tokio::test(start_paused = true)]
async fn action_settling_does_not_wait_the_yaml_delay_twice() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture::default());
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId": "chrome:fixture", "allowedTaps": ["left"], "settleMs": 450
    }))
    .unwrap();
    let timing = timing::TimingLog::new();
    crawl::scan(
        &robot,
        &opt,
        &AtomicBool::new(false),
        &mut map,
        &dir.0,
        &timing,
    )
    .await
    .unwrap();
    timing.save(&dir.0).await.unwrap();
    let phases: Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("timing.json")).unwrap()).unwrap();
    let phases = &phases["phases"];
    assert_eq!(map.transitions.len(), 1);
    assert!(phases["execute_action"]["totalMs"].as_u64().unwrap() >= 450);
    let post_action = phases["settle_after_action"]["totalMs"].as_u64().unwrap();
    assert!(
        (140..450).contains(&post_action),
        "post-action settling took {post_action} ms"
    );
    let yaml = std::fs::read_to_string(
        dir.0.join(
            &map.screens[0]
                .controls
                .iter()
                .find(|control| control.label == "Left")
                .unwrap()
                .evidence
                .as_ref()
                .unwrap()
                .yaml,
        ),
    )
    .unwrap();
    assert!(yaml.contains("ms: 450"));
}

#[tokio::test(start_paused = true)]
async fn maps_distinct_screens_and_restores_branch_with_test_input_only() {
    let dir = Temp::new();
    let robot = std::sync::Arc::new(Fixture::default());
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture",
        "allowedTaps":["left","right","back","same","delete"],
        "inputs":{"search":"synthetic-QC-value"},
        "settleMs":100
    }))
    .unwrap();
    let driver: std::sync::Arc<dyn Robot> = robot.clone();
    scan(&driver, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    let state = robot.0.lock().unwrap();
    assert!(state.entered || state.taps.contains(&"editor".to_owned()));
    assert!(state.resets > 0);
    assert_eq!(map.screens.len(), 4);
    let root = &map.screens[0];
    assert!(root.screenshot && dir.0.join("s1.png").exists());
    assert_eq!(
        root.controls
            .iter()
            .find(|c| c.label == "Delete data")
            .unwrap()
            .status,
        "no_change"
    );
    assert!(state.taps.iter().any(|tap| tap == "delete"));
    assert_eq!(
        root.controls
            .iter()
            .find(|c| c.label == "Search")
            .unwrap()
            .status,
        "observed"
    );
    let input = root.controls.iter().find(|c| c.label == "Search").unwrap();
    let evidence = input.evidence.as_ref().unwrap();
    let yaml = std::fs::read_to_string(dir.0.join(&evidence.yaml)).unwrap();
    assert!(!yaml.contains("synthetic-QC-value"));
    assert!(yaml.contains("DEVICE_BOT_MAP_INPUT"));
    assert!(dir.0.join(evidence.screenshot.as_ref().unwrap()).is_file());
    let same = map
        .screens
        .iter()
        .flat_map(|screen| &screen.controls)
        .find(|control| control.label == "Same state")
        .unwrap();
    assert_eq!(same.status, "no_change");
    assert!(
        dir.0
            .join(same.evidence.as_ref().unwrap().screenshot.as_ref().unwrap())
            .is_file()
    );
    assert_eq!(
        route_result(&map, "s1", "right").unwrap()["steps"][0]["label"],
        "Right"
    );
    assert!(
        !serde_json::to_string(&map)
            .unwrap()
            .contains("synthetic-QC-value")
    );

    map.source_routes.push(
        serde_json::from_value(json!({
            "title":"Candidate dialog","route":"/candidate","sourcePath":"src/router.dart",
            "line":4,"status":"entry_unknown","steps":[]
        }))
        .unwrap(),
    );
    assert_eq!(
        route_result(&map, "s1", "/candidate").unwrap()["status"],
        "source_only"
    );
    assert_eq!(
        route_result(&map, "s1", "/missing").unwrap()["status"],
        "no_observed_route"
    );
}

#[tokio::test(start_paused = true)]
async fn groups_checked_states_only_when_the_control_inventory_matches() {
    for (expand, show_extra_control_when_checked) in [(false, false), (true, false), (false, true)]
    {
        let dir = Temp::new();
        let robot: std::sync::Arc<dyn Robot> =
            std::sync::Arc::new(Fixture(Mutex::new(FixtureState {
                show_extra_control_when_checked,
                ..FixtureState::default()
            })));
        let mut map = graph();
        let opt: Options = serde_json::from_value(json!({
            "deviceId":"chrome:fixture",
            "allowedTaps":["toggle"],
            "expandSelectionStates":expand,
            "settleMs":100,
        }))
        .unwrap();
        scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
            .await
            .unwrap();
        let grouped = !expand && !show_extra_control_when_checked;
        assert_eq!(map.screens.len(), 2);
        assert_eq!(map.attempted, if grouped { 1 } else { 2 });
        assert_eq!(map.transitions.len(), map.attempted);
        assert_eq!(
            map.screens[1].same_controls_as.as_deref(),
            grouped.then_some("s1")
        );
        let toggle = map.screens[1]
            .controls
            .iter()
            .find(|control| control.label == "Toggle task")
            .unwrap();
        assert_eq!(
            toggle.status,
            if grouped { "same_controls" } else { "observed" }
        );
        assert!(map.screens[1].screenshot);
    }
}

#[tokio::test(start_paused = true)]
async fn waits_for_controls_on_a_cold_web_entry_and_reports_empty_inventory() {
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture",
        "allowedTaps":["left"],
        "settleMs":100,
    }))
    .unwrap();
    for empty_queries in [4, usize::MAX] {
        let dir = Temp::new();
        let robot: std::sync::Arc<dyn Robot> =
            std::sync::Arc::new(Fixture(Mutex::new(FixtureState {
                empty_queries,
                ..FixtureState::default()
            })));
        let mut map = graph();
        scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
            .await
            .unwrap();
        if empty_queries == 4 {
            assert_eq!(map.attempted, 1);
            assert_eq!(map.transitions.len(), 1);
            assert!(map.notes.is_empty());
        } else {
            assert_eq!(map.attempted, 0);
            assert!(map.screens[0].controls.is_empty());
            assert!(map.notes.iter().any(|note| note.starts_with("No controls")));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn reuses_observed_web_routes_to_restore_siblings_without_reloading() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture::default());
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture",
        "allowedTaps":["left", "right", "back", "same"],
        "settleMs":100,
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.attempted, 5);
    assert_eq!(map.transitions.len(), 4);
    assert_eq!(fixture.0.lock().unwrap().resets, 2);
}

#[tokio::test(start_paused = true)]
async fn a_slow_map_action_is_not_cut_off_by_a_second_case_deadline() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        tap_delay_ms: 46_000,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "resetAppId":"fixture.app",
        "allowedTaps":["left"], "settleMs":100
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(fixture.0.lock().unwrap().taps, ["left"]);
    assert_eq!(map.transitions.len(), 1);
    assert_eq!(map.screens[0].controls[0].status, "observed");
}

#[tokio::test(start_paused = true)]
async fn mobile_map_accepts_focus_confirmed_by_a_slow_native_observation() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        android: true,
        focused_tree_delay_ms: 31_000,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"android-fixture", "resetAppId":"fixture.app",
        "inputs":{"search":"synthetic-QC-value"}
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert!(fixture.0.lock().unwrap().entered);
    assert_eq!(map.transitions.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn mobile_input_confirms_the_unique_focused_copy_of_an_editor() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        duplicate_editor_after_focus: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "resetAppId":"fixture.app",
        "inputs":{"search":"synthetic-QC-value"}
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert!(fixture.0.lock().unwrap().entered);
}

#[tokio::test(start_paused = true)]
async fn resumes_the_same_map_after_mobile_transport_loss_without_resending_the_tap() {
    for (ios, device, recovered_device) in [
        (false, "android-fixture", "android-recovered"),
        (true, "ios-fixture", "ios-fixture"),
    ] {
        let dir = Temp::new();
        let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
            android: !ios,
            ios,
            disconnect_on_tap: Some("same"),
            extra_left_action: true,
            delay_android_foreground_on_launch: !ios,
            ..Default::default()
        })));
        let robot: Arc<dyn Robot> = fixture.clone();
        let mut map = graph();
        let opt: Options = serde_json::from_value(json!({
            "deviceId":device, "resetAppId":"fixture.app",
            "allowedTaps":["left","same","after"], "settleMs":100
        }))
        .unwrap();
        scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
            .await
            .unwrap();
        assert_eq!(fixture.0.lock().unwrap().recoveries, 1);
        assert_eq!(
            fixture.0.lock().unwrap().taps,
            ["left", "same", "left", "after"]
        );
        assert_eq!(map.device_id, recovered_device);
        assert_eq!(map.attempted, 3);
        assert_eq!(map.transitions.len(), 1);
        assert_eq!(map.transitions[0].label, "Left");
        assert!(
            map.notes
                .iter()
                .any(|note| note.contains("Continuing from verified screens"))
        );
        assert!(
            map.screens[1]
                .controls
                .iter()
                .any(|control| control.label == "Same state" && control.status == "failed")
        );
        assert!(
            map.screens[1].controls.iter().any(|control| {
                control.label == "After recovery" && control.status == "no_change"
            })
        );
    }
}

#[tokio::test(start_paused = true)]
async fn android_skips_an_unrestorable_branch_and_reuses_a_known_sibling_route() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        android: true,
        left_no_longer_reproducible: true,
        delay_android_foreground_on_launch: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"android-fixture",
        "resetAppId":"fixture.app",
        "allowedTaps":["left", "right", "back", "same"],
        "settleMs":100,
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.attempted, 3);
    assert_eq!(map.transitions.len(), 3);
    assert_eq!(fixture.0.lock().unwrap().resets, 2);
    assert!(map.notes.iter().any(|note| note.starts_with("s2:")));
    assert!(
        map.screens[1]
            .controls
            .iter()
            .all(|control| control.status == "skipped")
    );
    assert!(
        map.screens[2]
            .controls
            .iter()
            .any(|control| control.status == "observed")
    );
}

#[tokio::test(start_paused = true)]
async fn xctest_snapshot_ids_do_not_prevent_stable_label_replay() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture(Mutex::new(FixtureState {
        xctest_generation: Some(0),
        ..Default::default()
    })));
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture", "allowedTaps":["Left"], "settleMs":100,
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.attempted, 1);
    let controls = &map.screens[0].controls;
    assert!(
        controls
            .iter()
            .any(|c| c.label == "Left" && c.resource_id.is_none() && c.status == "observed")
    );
    assert!(!controls.iter().any(|c| c.label == "Banner"));
}

#[tokio::test(start_paused = true)]
async fn physical_ios_snapshot_uuids_do_not_prevent_entry_settling() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        xctest_uuid: true,
        xctest_generation: Some(0),
        ..Default::default()
    })));
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "allowedTaps":["Left"]
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    let root = &map.screens[0];
    assert!(
        root.controls
            .iter()
            .any(|c| c.label == "Left" && c.resource_id.is_none() && c.status == "observed")
    );
    assert!(!root.controls.iter().any(|c| c.label == "Banner"));
}

#[tokio::test(start_paused = true)]
async fn omitted_scan_time_limit_allows_a_scan_longer_than_eight_minutes() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture(Mutex::new(FixtureState {
        tree_delay_ms: 500_000,
        ..Default::default()
    })));
    let opt: Options = serde_json::from_value(json!({"deviceId":"chrome:fixture"})).unwrap();
    let mut map = graph();
    let start = tokio::time::Instant::now();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert!(start.elapsed() > Duration::from_secs(480));
    assert!(map.screens[0].screenshot);
}

#[tokio::test(start_paused = true)]
async fn ios_scoped_scan_reuses_post_action_snapshot_for_unchanged_screen() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        tree_delay_ms: 30,
        ..Default::default()
    })));
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "resetAppId":"fixture.app",
        "allowedTaps":["delete","toggle"]
    }))
    .unwrap();
    let mut map = graph();
    let timing = timing::TimingLog::new();
    crawl::scan(
        &robot,
        &opt,
        &AtomicBool::new(false),
        &mut map,
        &dir.0,
        &timing,
    )
    .await
    .unwrap();
    assert_eq!(map.attempted, 2);
    timing.save(&dir.0).await.unwrap();
    let timings: Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("timing.json")).unwrap()).unwrap();
    let restore = &timings["phases"]["restore_branch"];
    assert_eq!(restore["calls"], 2);
    assert_eq!(restore["totalMs"], 30);
}

#[tokio::test(start_paused = true)]
async fn ios_can_restore_an_observed_route_without_relaunching_the_app() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        ..Default::default()
    })));
    let robot: Arc<dyn Robot> = fixture.clone();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "allowedTaps":["left", "back", "same"], "settleMs":100,
    }))
    .unwrap();
    scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.attempted, 3);
    assert_eq!(map.transitions.len(), 2);
    assert_eq!(fixture.0.lock().unwrap().resets, 0);
}

#[tokio::test(start_paused = true)]
async fn ios_uses_runner_foreground_status_and_stops_after_leaving_the_app() {
    let dir = Temp::new();
    let robot: Arc<dyn Robot> = Arc::new(Fixture(Mutex::new(FixtureState {
        ios: true,
        ..Default::default()
    })));
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"ios-fixture", "resetAppId":"fixture.app",
        "allowedTaps":["external"], "settleMs":100,
    }))
    .unwrap();
    let error = scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap_err();
    assert!(error.contains("Could not verify the iOS foreground app"));
    assert_eq!(map.attempted, 1);
    assert_eq!(map.screens.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn never_inspects_or_records_a_tab_outside_starting_origin() {
    let dir = Temp::new();
    let robot = Fixture::default();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture","allowedTaps":["external"],"settleMs":100,
    }))
    .unwrap();
    let driver: std::sync::Arc<dyn Robot> = std::sync::Arc::new(robot);
    let result = scan(&driver, &opt, &AtomicBool::new(false), &mut map, &dir.0).await;
    assert!(result.unwrap_err().contains("left the starting origin"));
    assert_eq!(map.screens.len(), 1);
    assert!(!dir.0.join("actions/s1-c5/result.png").exists());
}

#[test]
fn rejects_noncanonical_map_path_and_prevents_parallel_scans() {
    let root = Path::new("/tmp/maps");
    assert!(map_dir(root, "../../anything").is_err());
    let (lease, _stop) = acquire("windows:41:42", "one").unwrap();
    assert!(acquire("windows:31:32", "two").is_err());
    drop(lease);
    assert!(acquire("windows:31:32", "two").is_ok());
}

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

mod desktop_fixture;
mod mobile;
mod navigation;
mod viewport_discovery;

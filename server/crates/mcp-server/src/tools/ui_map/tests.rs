use std::{path::PathBuf, sync::Mutex};

use async_trait::async_trait;
use robot_core::{Bounds, ElementType, Point, RobotDevice, RobotError, UiElement};

use super::*;

#[derive(Default)]
struct Fixture(Mutex<FixtureState>);

#[derive(Default)]
struct FixtureState {
    page: usize,
    android: bool,
    left_no_longer_reproducible: bool,
    focus: bool,
    entered: bool,
    checked: bool,
    show_extra_control_when_checked: bool,
    empty_queries: usize,
    taps: Vec<String>,
    resets: usize,
}

impl Fixture {
    fn reset(&self) {
        let mut state = self.0.lock().unwrap();
        state.page = 0;
        state.entered = false;
        state.checked = false;
        state.focus = false;
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
        let android = self.0.lock().unwrap().android;
        Ok(vec![RobotDevice {
            id: if android {
                "android-fixture"
            } else {
                "chrome:fixture"
            }
            .into(),
            name: "Fixture".into(),
            platform: if android {
                robot_core::Platform::Android
            } else {
                robot_core::Platform::Chrome
            },
            state: robot_core::DeviceState::Available,
        }])
    }

    async fn stream_context(&self, _: &str) -> Result<Value, RobotError> {
        let state = self.0.lock().unwrap();
        if state.android {
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

    async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
        Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec())
    }

    async fn tap(&self, _: &str, point: Point) -> Result<(), RobotError> {
        let mut state = self.0.lock().unwrap();
        let action = match (state.page, point.x as i32) {
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
            (0, 40) => panic!("blocked Delete must never be tapped"),
            (0, 50) => {
                state.page = 3;
                "external"
            }
            (0, 70) => {
                state.checked = !state.checked;
                "toggle"
            }
            (1 | 2, 10) => {
                state.page = 0;
                "back"
            }
            (1, 20) => "same",
            _ => return Err(RobotError::CommandFailed("Unexpected tap".into())),
        };
        state.focus = action == "editor";
        state.taps.push(action.into());
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

    async fn swipe(&self, _: &str, _: Point, _: Point, _: Option<u64>) -> Result<(), RobotError> {
        unreachable!()
    }
    async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        unreachable!()
    }
    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        unreachable!()
    }

    async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
        let mut s = self.0.lock().unwrap();
        if s.empty_queries > 0 {
            s.empty_queries -= 1;
            return Ok(Vec::new());
        }
        let rows = match s.page {
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
            1 => vec![
                node("Back", "back", 10, ElementType::Button),
                node("Same state", "same", 20, ElementType::Button),
            ],
            2 => vec![node("Back", "back", 10, ElementType::Button)],
            _ => panic!("must stop before inspecting outside origin"),
        };
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
    crawl::scan(&driver, &opt, &AtomicBool::new(false), &mut map, &dir.0)
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
        "skipped"
    );
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
        crawl::scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
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
        crawl::scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
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
    crawl::scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
        .await
        .unwrap();
    assert_eq!(map.attempted, 5);
    assert_eq!(map.transitions.len(), 4);
    assert_eq!(fixture.0.lock().unwrap().resets, 2);
}

#[tokio::test(start_paused = true)]
async fn android_skips_an_unrestorable_branch_and_reuses_a_known_sibling_route() {
    let dir = Temp::new();
    let fixture = Arc::new(Fixture(Mutex::new(FixtureState {
        android: true,
        left_no_longer_reproducible: true,
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
    crawl::scan(&robot, &opt, &AtomicBool::new(false), &mut map, &dir.0)
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
async fn never_inspects_or_records_a_tab_outside_starting_origin() {
    let dir = Temp::new();
    let robot = Fixture::default();
    let mut map = graph();
    let opt: Options = serde_json::from_value(json!({
        "deviceId":"chrome:fixture","allowedTaps":["external"],"settleMs":100,
    }))
    .unwrap();
    let driver: std::sync::Arc<dyn Robot> = std::sync::Arc::new(robot);
    let result = crawl::scan(&driver, &opt, &AtomicBool::new(false), &mut map, &dir.0).await;
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

use super::*;

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

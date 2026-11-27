use super::*;

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

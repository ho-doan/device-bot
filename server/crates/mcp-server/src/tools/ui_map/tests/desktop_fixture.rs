use super::*;

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

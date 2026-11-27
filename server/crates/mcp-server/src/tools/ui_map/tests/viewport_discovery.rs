use super::*;

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

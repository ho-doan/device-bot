use super::*;

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires disposable scripts/fixtures/macos_input.swift and Accessibility"]
async fn live_return_submits_focused_field_instead_of_inserting_a_newline() {
    let path = std::env::var("DEVICE_BOT_MACOS_RETURN_TEST_STATE").unwrap();
    let read_state =
        || -> Value { serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap() };
    let state = read_state();
    let target = Target {
        pid: state["processId"].as_i64().unwrap() as i32,
        window: state["windowId"].as_u64().unwrap() as u32,
    };
    assert!(
        native_listed_windows()
            .unwrap()
            .iter()
            .any(|window| window.process_id == target.pid as u32
                && window.window_id == target.window.to_string()
                && window.title == "Device Bot Input Fixture")
    );
    let robot = MacosRobot::new(std::env::temp_dir());
    let id = format!("macos:{}:{}", target.pid, target.window);
    robot
        .live_geometry
        .lock()
        .await
        .insert(id.clone(), geometry(target).unwrap());
    let field = robot
        .list_elements(&id)
        .await
        .unwrap()
        .into_iter()
        .find(|element| element.content_description.as_deref() == Some("Notes input"))
        .expect("disposable fixture input");
    robot
        .tap(
            &id,
            Point {
                x: (field.bounds.left + field.bounds.right) as f64 / 2.,
                y: (field.bounds.top + field.bounds.bottom) as f64 / 2.,
            },
        )
        .await
        .unwrap();
    robot.type_text(&id, "\n").await.unwrap();
    for _ in 0..20 {
        if read_state()["returnCount"].as_u64() == Some(1) {
            assert_eq!(read_state()["text"], "");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!(
        "Return did not trigger the field's submit action: {}",
        read_state()
    );
}

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

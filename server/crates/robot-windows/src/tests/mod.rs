use super::*;

#[test]
fn window_titles_with_unpaired_utf16_do_not_break_discovery() {
    let windows = parse_window_processes(br#"[{"processId":20,"name":"app","windowId":"111","title":"Broken \ud83d title \ude00; emoji \ud83d\ude00"}]"#).unwrap();
    assert_eq!(windows[0].title, "Broken \u{fffd} title \u{fffd}; emoji 😀");
    assert_eq!(windows[0].window_id, "111");
}

#[test]
fn window_list_keeps_titles_when_powershell_bytes_are_not_utf8() {
    let windows = parse_window_processes(
        b"[{\"processId\":7,\"name\":\"Notepad\",\"windowId\":\"9\",\"title\":\"Caf\xe9 notes\"},{\"processId\":8,\"name\":\"Calculator\",\"windowId\":\"10\",\"title\":\"Calculator\"}]",
    )
    .unwrap();
    assert_eq!(windows[0].title, "Caf\u{fffd} notes");
    assert_eq!(windows[1].name, "Calculator");
    let script = utf8_powershell(live::window_list_script(None));
    assert!(script.starts_with("$OutputEncoding = New-Object System.Text.UTF8Encoding $false\n"));
    assert!(script.contains("ConvertTo-Json -InputObject $rows"));
}

#[test]
fn powershell_json_keeps_valid_escapes_and_rejects_broken_structure() {
    let value =
        parse_powershell_json(br#"{"title":"literal \\ud83d and \"quoted\"; \ud83d\ude00"}"#)
            .unwrap();
    assert_eq!(value["title"], "literal \\ud83d and \"quoted\"; 😀");
    assert!(parse_powershell_json(br#"{"title":"\ud800",oops}"#).is_err());
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(b"[]");
    assert_eq!(parse_powershell_json(&bom).unwrap(), serde_json::json!([]));
}

#[tokio::test]
async fn target_selection_is_isolated_per_mcp_session() {
    let robot = WindowsRobot::new("artifacts");
    let first = robot
        .fork_session()
        .expect("desktop sessions must have independent targets");
    let second = robot.fork_session().expect("second session");
    assert_eq!(Arc::strong_count(&robot.active_process_id), 1);
    drop((first, second));
}

#[test]
fn powershell_literals_are_quoted() {
    assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
}

#[test]
fn parses_process_id_output() {
    let windows = parse_window_processes(
        br#"[{"processId":20,"name":"chrome","windowId":"111","title":"Tab A"},{"processId":8,"name":"Calculator","windowId":"222","title":""}]"#,
    )
    .unwrap();
    assert_eq!(windows[0].process_id, 20);
    assert_eq!(windows[1].title, "");
    assert_eq!(windows[1].name, "Calculator");

    assert_eq!(parse_process_id(b"\r\n1234\r\n").unwrap(), 1234);
    assert_eq!(parse_process_id("\u{feff}5678\n".as_bytes()).unwrap(), 5678);
    assert!(parse_process_id(b"not-a-process").is_err());
}

#[test]
fn uia_script_uses_raw_view_and_process_condition() {
    let script = build_uia_script(Some(1234), None);
    assert!(script.contains("UIAutomationClient"));
    assert!(script.contains("TreeWalker]::RawViewWalker"));
    assert!(script.contains("ProcessIdProperty"));
    assert!(script.contains("[uint32]1234"));
}

#[test]
fn uia_script_bounds_native_results() {
    let script = build_uia_script(None, Some(20));
    assert!(script.contains("$maxResults = [int]20"));
    assert!(script.contains("$maxNodes = [int]5000"));
    assert!(script.contains("$maxDepth = [int]128"));
}

#[test]
fn parses_uia_payload_into_portable_elements() {
    let elements = parse_uia_elements(
        br#"[{"className":"button","resourceId":"save","text":"Save","contentDescription":"Save","left":10,"top":20,"right":70,"bottom":50,"clickable":true,"enabled":false}]"#,
    )
    .unwrap();
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].class_name, "button");
    assert_eq!(elements[0].resource_id.as_deref(), Some("save"));
    assert_eq!(
        elements[0].bounds,
        Bounds {
            left: 10,
            top: 20,
            right: 70,
            bottom: 50
        }
    );
    assert!(elements[0].clickable);
    assert!(!elements[0].enabled);
}

#[test]
fn parses_uia_payload_with_a_utf8_bom() {
    let mut output = vec![0xef, 0xbb, 0xbf];
    output.extend_from_slice(br#"[]"#);
    assert!(parse_uia_elements(&output).unwrap().is_empty());
}

#[test]
fn native_payload_preserves_empty_editor_values_and_unknown_states() {
    let elements = parse_uia_elements(br#"[{"className":"textField","left":-120,"top":20,"right":-20,"bottom":50,"value":"","focused":null,"checked":null,"selected":null}]"#).unwrap();
    assert_eq!(elements[0].value.as_deref(), Some(""));
    assert_eq!(elements[0].focused, None);
    assert_eq!(elements[0].checked, None);
    assert_eq!(elements[0].selected, None);
    assert_eq!(elements[0].bounds.left, -120);
    assert!(parse_uia_elements(br#"[{"className":"textField","left":0}]"#).is_err());
}

#[tokio::test]
async fn zero_max_results_does_not_start_powershell() {
    let robot = WindowsRobot::new("artifacts");
    assert!(robot.uia_elements(Some(0)).await.unwrap().is_empty());
}

// Run against a started, visible examples/flutter_app Windows build:
// DEVICE_BOT_FLUTTER_SMOKE_PID=<pid> cargo test -p robot-windows flutter_live -- --ignored
#[cfg(target_os = "windows")]
#[tokio::test]
#[ignore = "requires a running Flutter sample and an interactive desktop"]
async fn flutter_live_widget_tree_and_native_input() {
    let process_id: u32 = std::env::var("DEVICE_BOT_FLUTTER_SMOKE_PID")
        .expect("set DEVICE_BOT_FLUTTER_SMOKE_PID to the running sample PID")
        .parse()
        .unwrap();
    let robot = WindowsRobot::new("artifacts");
    *robot.active_process_id.lock().await = Some(process_id);
    let query = robot_core::ElementQuery {
        element_type: Some(robot_core::ElementType::TextField),
        filter: Some("email-field".into()),
        max_results: Some(1),
        web_context: None,
        include_frames: false,
        exclude_system_keyboard: false,
    };
    let elements = robot_core::apply_element_query(
        robot.list_flutter_elements(DEVICE_ID).await.unwrap(),
        &query,
    );
    assert_eq!(elements.len(), 1);
    let email = &elements[0];
    assert_eq!(email.resource_id.as_deref(), Some("email-field"));
    robot
        .tap(
            DEVICE_ID,
            Point {
                x: f64::from(email.bounds.left + email.bounds.right) / 2.0,
                y: f64::from(email.bounds.top + email.bounds.bottom) / 2.0,
            },
        )
        .await
        .unwrap();
    let literal = "+rust Ỏbit123! ^%{}[]  ";
    robot.type_text(DEVICE_ID, literal).await.unwrap();
    let updated = robot_core::apply_element_query(
        robot.list_flutter_elements(DEVICE_ID).await.unwrap(),
        &query,
    );
    assert_eq!(updated[0].focused, Some(true));
    assert!(updated[0].value.as_deref().unwrap().contains(literal));
    // Native UIA remains independent of the optional Flutter bridge.
    let manifest = flutter_bridge::directory().join(format!("{process_id}.json"));
    let saved = manifest.with_extension("smoke-backup");
    tokio::fs::rename(&manifest, &saved).await.unwrap();
    let native = robot.list_elements(DEVICE_ID).await;
    tokio::fs::rename(&saved, &manifest).await.unwrap();
    assert!(native.is_ok(), "native UIA fallback failed: {native:?}");
}

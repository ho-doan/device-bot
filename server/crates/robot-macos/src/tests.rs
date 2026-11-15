//! Driver contract, app selection, and activation regressions.

use super::windows::{DesktopWindow, running_apps_from_windows, windows_for_process};

use super::*;

#[cfg(target_os = "macos")]
#[tokio::test]
async fn activation_waits_until_foreground_is_observed_and_times_out_if_it_never_arrives() {
    let mut observations = 0;
    wait_for_activation(
        || {
            observations += 1;
            std::future::ready(if observations < 3 {
                Err(RobotError::InvalidInput("not foreground yet".into()))
            } else {
                Ok(())
            })
        },
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(observations, 3);

    let error = wait_for_activation(
        || std::future::ready(Err(RobotError::InvalidInput("still background".into()))),
        Duration::from_millis(1),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, RobotError::CommandFailed(message) if message.contains("did not activate"))
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn host_scroll_reaches_delta_validation_without_a_live_window() {
    let error = MacosRobot::new("artifacts")
        .scroll("macos-host", Point { x: 0., y: 0. }, 0., f64::NAN)
        .await
        .unwrap_err();
    // Reject before posting a Quartz event, so this regression needs no GUI input.
    assert!(
        matches!(error, RobotError::InvalidInput(message) if message == "Invalid scroll delta")
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn window_live_ids_reach_the_real_macos_driver() {
    let robot = MacosRobot::new("artifacts");
    assert!(robot.require_device("macos:38553:2219").is_ok());
    let registry = robot_core::RobotRegistry::new(vec![Arc::new(robot) as Arc<dyn Robot>]);
    assert!(
        registry
            .negotiate_native_video("macos:38553:2219", "")
            .await
            .unwrap()
            .is_none()
    );
    // A gone window must reach geometry validation, never fail registry routing.
    assert!(matches!(
        registry.stream_context("macos:2147483647:4294967295").await,
        Err(RobotError::InvalidInput(_))
    ));
}

#[tokio::test]
async fn target_selection_is_isolated_per_mcp_session() {
    let robot = MacosRobot::new("artifacts");
    let first = robot
        .fork_session()
        .expect("desktop sessions must have independent targets");
    let second = robot.fork_session().expect("second session");
    first
        .select_ui_target(
            "macos-host",
            UiTarget {
                process_id: Some(123),
                window_id: Some("window-a".into()),
            },
        )
        .await
        .unwrap();
    second
        .select_ui_target(
            "macos-host",
            UiTarget {
                process_id: Some(456),
                window_id: None,
            },
        )
        .await
        .unwrap();
    assert!(robot.active_process_id.lock().await.is_none());
    assert!(
        robot
            .select_ui_target("macos-host", UiTarget::default())
            .await
            .is_err()
    );
    first
        .select_ui_target("macos-host", UiTarget::default())
        .await
        .unwrap();
}

#[test]
fn rejects_other_device_ids() {
    assert!(
        MacosRobot::new("artifacts")
            .require_device("another-device")
            .is_err()
    );
}

#[test]
fn parses_system_events_elements() {
    let elements = parse_accessibility_elements(
        "AXButton\x1fNew Tab\x1fOpen a new tab\x1f\x1f12\x1f34\x1f24\x1f24",
    );
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].text.as_deref(), Some("New Tab"));
    assert_eq!(elements[0].bounds.right, 36);
    assert!(elements[0].clickable);
}

#[test]
fn running_apps_are_unique_processes_sorted_by_name() {
    let windows = [
        DesktopWindow {
            process_id: 20,
            window_id: "1".into(),
            owner: "Chrome".into(),
            title: "Tab A".into(),
        },
        DesktopWindow {
            process_id: 20,
            window_id: "2".into(),
            owner: "Chrome".into(),
            title: "Tab B".into(),
        },
        DesktopWindow {
            process_id: 8,
            window_id: "3".into(),
            owner: "Calculator".into(),
            title: String::new(),
        },
    ];
    let apps = running_apps_from_windows(&windows);
    assert_eq!(
        apps,
        [
            RunningApp {
                process_id: 8,
                name: "Calculator".into(),
                bundle_id: None,
            },
            RunningApp {
                process_id: 20,
                name: "Chrome".into(),
                bundle_id: None,
            },
        ]
    );
    assert_eq!(windows_for_process(&windows, Some(20)).len(), 2);
    assert_eq!(
        windows_for_process(&windows, Some(8))[0].title,
        "Calculator"
    );
}

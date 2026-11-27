use super::*;

async fn delayed_native_query(
    delay_seconds: u64,
    step_timeout_ms: Option<u64>,
    case_seconds: u64,
) -> (robot_flow::FlowReport, std::time::Duration) {
    let robot = Arc::new(RecordingRobot {
        query_delay: std::time::Duration::from_secs(delay_seconds),
        ..Default::default()
    });
    let timeout = step_timeout_ms
        .map(|ms| format!(", timeout_ms: {ms}"))
        .unwrap_or_default();
    let flow = robot_flow::FlowDocument::from_yaml(&format!(
        r#"
version: 2
name: native_query_deadline
steps:
  - select_device: {{id: emulator-5554}}
  - id: query-native-ui
    assert_element: {{label: Submit{timeout}}}
"#
    ))
    .unwrap();
    let directory =
        std::env::temp_dir().join(format!("native-query-deadline-{}", uuid::Uuid::new_v4()));
    let mut runner = robot_flow::FlowRunner::new(
        Some(Box::new(RunnerMcp(Server::new(robot)))),
        directory.clone(),
    );
    let started = tokio::time::Instant::now();
    let report = runner
        .run(
            &flow,
            &robot_flow::Profile::default(),
            &[],
            std::time::Duration::from_secs(case_seconds),
        )
        .await;
    let _ = std::fs::remove_dir_all(directory);
    (report, started.elapsed())
}

#[tokio::test(start_paused = true)]
async fn yaml_native_query_honors_extended_step_deadline() {
    // Native startup + query can take more than 60s while remaining inside the
    // YAML step's explicit 120s budget. Keep the real runner and MCP dispatcher.
    let (report, elapsed) = delayed_native_query(61, Some(120_000), 180).await;
    assert_eq!(report.status, "PASS", "{report:?}");
    assert_eq!(elapsed, std::time::Duration::from_secs(61));
}

#[tokio::test(start_paused = true)]
async fn yaml_native_query_remains_bounded_by_default_step_deadline() {
    let (report, elapsed) = delayed_native_query(61, None, 180).await;
    assert_eq!(report.status, "FAIL");
    assert!(report.error.unwrap().contains("bounded wait expired"));
    assert_eq!(elapsed, std::time::Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn yaml_native_query_remains_bounded_by_explicit_step_deadline() {
    let (report, elapsed) = delayed_native_query(121, Some(120_000), 180).await;
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.steps.last().unwrap().id, "query-native-ui");
    assert_eq!(elapsed, std::time::Duration::from_secs(120));
}

#[tokio::test(start_paused = true)]
async fn yaml_native_query_remains_bounded_by_case_deadline() {
    let (report, elapsed) = delayed_native_query(61, Some(120_000), 5).await;
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.error_kind.as_deref(), Some("timeout"));
    assert_eq!(report.steps.last().unwrap().id, "query-native-ui");
    assert_eq!(elapsed, std::time::Duration::from_secs(5));
}

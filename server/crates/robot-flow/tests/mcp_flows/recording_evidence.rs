use super::*;

#[tokio::test]
async fn stop_recording_rejects_corrupt_video_artifact() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("corrupt.mp4");
    std::fs::write(&video, b"not a video").unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        ..State::default()
    }));
    let scenario = flow("  - start_screen_recording: {}\n  - stop_screen_recording: {}");
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&scenario, &Profile::default(), &[], Duration::from_secs(10))
        .await;
    assert_eq!(report.error_kind.as_deref(), Some("artifact"));
    assert!(!tmp.0.join("video.mp4").exists());
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c["params"]["name"] == "mobile_stop_screen_recording")
            .count(),
        1
    );
}

#[tokio::test]
async fn recording_validation_uses_scoped_ffmpeg_path() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let video = tmp.0.join("source.mp4");
    std::fs::write(&video, include_bytes!("../fixtures/tiny.mp4")).unwrap();
    let state = Arc::new(Mutex::new(State {
        video_path: Some(video),
        ..State::default()
    }));
    let scenario = flow("  - start_screen_recording: {}\n  - stop_screen_recording: {}");
    let mut runner = FlowRunner::new(Some(Box::new(Fake(state))), tmp.0.clone());
    runner.set_environment(std::collections::BTreeMap::from([(
        "DEVICE_BOT_FFMPEG_PATH".into(),
        tmp.0.join("missing-ffmpeg").to_string_lossy().into_owned(),
    )]));
    let report = runner
        .run(&scenario, &Profile::default(), &[], Duration::from_secs(10))
        .await;
    assert_eq!(report.error_kind.as_deref(), Some("artifact"));
    assert!(
        report
            .error
            .unwrap()
            .contains("could not start video validation")
    );
    assert!(!tmp.0.join("video.mp4").exists());
}

#[tokio::test]
async fn observation_redaction_secrets_do_not_enter_expanded_flow_snapshot() {
    // Exercise the ordinary process-environment path without modifying it.
    let name = "PATH";
    let secret = std::env::var(name).expect("PATH is needed to run the test suite");
    assert!(!secret.is_empty());
    let scenario = flow(&format!(
        "  - call_tool:\n      name: mobile_start_observation\n      arguments:\n        deviceId: device-1\n        redact: ['${{env:{name}}}', literal-private-token]\n"
    ));
    let state = Arc::new(Mutex::new(State {
        schemas: Some(vec![
            json!({"name":"mobile_list_available_devices","inputSchema":{}}),
            json!({"name":"mobile_start_observation","inputSchema":{}}),
        ]),
        ..State::default()
    }));
    let tmp = Temp::new();
    let report = FlowRunner::new(Some(Box::new(Fake(state.clone()))), tmp.0.clone())
        .run(&scenario, &Profile::default(), &[], Duration::from_secs(3))
        .await;
    assert_eq!(report.status, "PASS", "{:?}", report.error);
    let snapshot = std::fs::read_to_string(tmp.0.join("expanded-flow.yaml")).unwrap();
    assert!(!snapshot.contains(&secret));
    assert!(!snapshot.contains("literal-private-token"));
    assert!(snapshot.contains("<REDACTED>"));
    let calls = state.lock().unwrap();
    let args = &calls
        .calls
        .iter()
        .find(|call| call["params"]["name"] == "mobile_start_observation")
        .unwrap()["params"]["arguments"];
    assert_eq!(args["redact"][0], secret);
}

#[test]
fn cli_persists_redacted_scenario_instead_of_raw_literal_input() {
    let tmp = Temp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let scenario = tmp.0.join("literal-source.yaml");
    std::fs::write(
        &scenario,
        "version: 2\nname: privacy\nsteps:\n  - type_text: {text: cli-private-literal-482}\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_robot-flow"))
        .args(["--dry-run", "--artifacts"])
        .arg(tmp.0.join("evidence"))
        .arg(&scenario)
        .env("DEVICE_BOT_REPORT_DATA_DIR", tmp.0.join("history"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fn inspect(path: &std::path::Path) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                inspect(&entry.unwrap().path());
            }
        } else if let Ok(text) = std::fs::read_to_string(path) {
            assert!(
                !text.contains("cli-private-literal-482"),
                "secret in {}",
                path.display()
            );
        }
    }
    inspect(&tmp.0.join("evidence"));
    inspect(&tmp.0.join("history"));
}

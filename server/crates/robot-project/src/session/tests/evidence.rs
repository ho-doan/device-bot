use super::*;

#[tokio::test]
async fn dotenv_input_is_redacted_without_scrubbing_unreferenced_values() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - id: step-1\n    type_text: {text: '${env:DEVICE_PROJECT_TEST_SECRET_482}'}\n",
        PROFILES,
    );
    let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
        .unwrap()
        .replacen("name: test", "name: test\nenv_file: .env", 1);
    fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
    fs::write(
        fixture.root.join(".env"),
        "DEVICE_PROJECT_TEST_SECRET_482=private-value-482\nDEVICE_PROJECT_TEST_UNUSED_482=1\n",
    )
    .unwrap();
    let factory = Factory::default();
    let output = fixture.root.join("out");
    let result = run(
        &fixture.load(),
        &factory,
        &output,
        &[],
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(result.reports.len(), 2);
    let evidence = fs::read_to_string(output.join("case-001/expanded-flow.yaml")).unwrap();
    assert!(!evidence.contains("private-value-482"));
    assert!(evidence.contains("step-1"));
}
#[tokio::test]
async fn completed_case_evidence_survives_cleanup_failure() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        fail_close: true,
        ..Factory::default()
    };
    let output = fixture.root.join("out");
    assert!(
        run(
            &fixture.load(),
            &factory,
            &output,
            &[],
            Duration::from_secs(5)
        )
        .await
        .is_err()
    );
    assert!(output.join("case-001/report.json").is_file());
    assert!(output.join("case-001/junit.xml").is_file());
    assert!(output.join("case-002/report.json").is_file());
}

#[tokio::test]
async fn compile_error_redacts_referenced_environment_values() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - platform: '${env:DEVICE_PROJECT_TEST_BAD_PLATFORM_482}'\n    tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
        .unwrap()
        .replacen("name: test", "name: test\nenv_file: .env", 1);
    fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
    fs::write(
        fixture.root.join(".env"),
        "DEVICE_PROJECT_TEST_BAD_PLATFORM_482=private-invalid-platform-482\n",
    )
    .unwrap();
    let error = match run(
        &fixture.load(),
        &Factory::default(),
        &fixture.root.join("out"),
        &[],
        Duration::from_secs(5),
    )
    .await
    {
        Ok(_) => panic!("expected compile failure"),
        Err(error) => error.to_string(),
    };
    assert!(!error.contains("private-invalid-platform-482"));
    assert!(error.contains("<REDACTED>"));
}
#[tokio::test]
async fn capability_and_host_failures_stop_before_url_navigation() {
    for (status, host, expected_error) in [
        (
            Some("blocked"),
            None,
            "resource a requires input, capability status blocked",
        ),
        (
            Some("unsupported"),
            None,
            "resource a requires input, capability status unsupported",
        ),
        (
            Some("unknown"),
            None,
            "resource a requires input, capability status unknown",
        ),
        (
            None,
            Some("windows"),
            "resource a host OS does not match declared host",
        ),
    ] {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
            .unwrap()
            .replacen(
                "    timeout_ms: 1000",
                "    requires: [input]\n    timeout_ms: 1000",
                1,
            );
        fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
        let factory = Factory {
            capability_status: status.map(str::to_owned),
            host_os: host.map(str::to_owned),
            ..Factory::default()
        };
        let result = run(
            &fixture.load(),
            &factory,
            &fixture.root.join("out"),
            &[],
            Duration::from_secs(5),
        )
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("unsupported or mismatched targets must fail setup"),
        };
        assert!(error.to_string().contains(expected_error), "{error}");
        let events = factory.events.lock().unwrap();
        assert!(
            !events
                .iter()
                .any(|v| v.starts_with("mobile_open_url") || v.starts_with("mobile_click"))
        );
        assert_eq!(events.iter().filter(|v| v.starts_with("close:")).count(), 2);
    }
}

#[tokio::test]
async fn literal_input_is_not_copied_into_evidence_or_history() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - type_text: {text: literal-private-value-482}\n",
        PROFILES,
    );
    let factory = Factory::default();
    let output = fixture.root.join("out");
    let history = fixture.root.join("history");
    run_with_history(
        &fixture.load(),
        &factory,
        &output,
        &[],
        Duration::from_secs(5),
        Some(&history),
    )
    .await
    .unwrap();
    fn inspect(path: &Path) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                inspect(&entry.unwrap().path());
            }
        } else if let Ok(value) = fs::read_to_string(path) {
            assert!(
                !value.contains("literal-private-value-482"),
                "secret in {}",
                path.display()
            );
        }
    }
    inspect(&output);
    inspect(&history);
}

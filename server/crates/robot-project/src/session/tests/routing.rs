use super::*;

#[tokio::test]
async fn router_serializes_input_calls_across_resource_sessions() {
    let state = RequestConcurrency::default();
    let sessions: Shared = Arc::new(Mutex::new(BTreeMap::from([
        (
            "project:a".into(),
            Connected {
                client: Box::new(ConcurrentFake {
                    resource: "a".into(),
                    state: state.clone(),
                }),
                real_id: "native-a".into(),
                device: json!({"id":"project:a"}),
            },
        ),
        (
            "project:b".into(),
            Connected {
                client: Box::new(ConcurrentFake {
                    resource: "b".into(),
                    state: state.clone(),
                }),
                real_id: "native-b".into(),
                device: json!({"id":"project:b"}),
            },
        ),
    ])));
    let mut router_a = Router {
        sessions: Arc::clone(&sessions),
        devices: BTreeMap::new(),
        tools: BTreeMap::new(),
    };
    let mut router_b = Router {
        sessions,
        devices: BTreeMap::new(),
        tools: BTreeMap::new(),
    };

    let (result_a, result_b) = tokio::join!(
        router_a.request(
            "tools/call",
            json!({"name":"mobile_click_on_screen_at_coordinates","arguments":{"deviceId":"project:a","x":1,"y":1}}),
        ),
        router_b.request(
            "tools/call",
            json!({"name":"mobile_click_on_screen_at_coordinates","arguments":{"deviceId":"project:b","x":2,"y":2}}),
        ),
    );

    assert!(result_a.is_ok());
    assert!(result_b.is_ok());
    assert_eq!(state.maximum.load(Ordering::SeqCst), 1);
    let mut calls = state.calls.lock().unwrap().clone();
    calls.sort();
    assert_eq!(
        calls,
        vec![
            ("a".to_owned(), "native-a".to_owned()),
            ("b".to_owned(), "native-b".to_owned()),
        ]
    );
}

#[tokio::test]
async fn proxy_configures_only_its_owned_resource_before_boot() {
    let fixture = Fixture::new(
        "version: 2\nname: wait\nsteps:\n  - wait: {ms: 0}\n",
        PROFILES,
    );
    let path = fixture.root.join("project.yaml");
    let manifest = fs::read_to_string(&path).unwrap();
    let configured = manifest.replace(
        "    prepare: {kind: launch, url: 'http://example.test/a'}",
        "    web_proxy: {upstream: '${env:PROXY_SPIKE_ADDR}'}\n    prepare: {kind: launch, url: 'http://example.test/a'}",
    );
    let configured = configured.replace("kind: project\n", "kind: project\nenv_file: .env\n");
    fs::write(&path, &configured).unwrap();
    fs::write(
        fixture.root.join(".env"),
        "PROXY_SPIKE_ADDR=127.0.0.1:8080\n",
    )
    .unwrap();
    let project = fixture.load();

    for missing_web_proxy in [false, true] {
        let factory = Factory {
            missing_web_proxy,
            ..Factory::default()
        };
        let outcome = run(
            &project,
            &factory,
            &fixture.root.join(format!("run-{missing_web_proxy}")),
            &[],
            Duration::from_secs(3),
        )
        .await;
        let events = factory.events.lock().unwrap();
        let configure = events
            .iter()
            .position(|e| e == "mobile_web_test_proxy:a:chrome:browser");
        let boot = events
            .iter()
            .position(|e| e == "mobile_boot_device:a:chrome:browser");
        if missing_web_proxy {
            assert!(
                outcome
                    .err()
                    .expect("missing proxy tool must fail")
                    .to_string()
                    .contains("requires unavailable tool mobile_web_test_proxy")
            );
            assert!(configure.is_none() && boot.is_none());
        } else {
            assert!(outcome.is_ok(), "{:?}", outcome.err());
            assert!(configure.unwrap() < boot.unwrap(), "{events:?}");
            assert!(
                !events
                    .iter()
                    .any(|e| e.starts_with("mobile_web_test_proxy:b:"))
            );
        }
    }

    fs::write(
        &path,
        configured.replace(
            "prepare: {kind: launch, url: 'http://example.test/a'}",
            "prepare: {kind: attach}",
        ),
    )
    .unwrap();
    assert!(
        Project::load(&fixture.root)
            .unwrap_err()
            .to_string()
            .contains("project-owned launched Chrome")
    );
}

#[tokio::test]
async fn launched_chrome_waits_for_discovery_without_replaying_the_launch() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        discovery_a: Discovery::Delayed,
        ..Default::default()
    };
    let output = fixture.root.join("out");
    let result = run(
        &fixture.load(),
        &factory,
        &output,
        &[],
        Duration::from_secs(5),
    )
    .await
    .expect("the project should wait until the launched page is discoverable");
    assert_eq!(result.reports.len(), 2);
    assert!(result.reports.iter().all(|report| report.status == "PASS"));
    let events = factory.events.lock().unwrap();
    let browser_a: Vec<_> = events
        .iter()
        .filter(|event| event.split(':').nth(1) == Some("a"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        browser_a,
        [
            "connect:a",
            "mobile_boot_device:a:chrome:browser",
            "mobile_list_available_devices:a:",
            "mobile_list_available_devices:a:",
            "mobile_list_available_devices:a:",
            "mobile_get_capabilities:a:chrome:same",
            "mobile_open_url:a:chrome:same",
            "mobile_click_on_screen_at_coordinates:a:chrome:same",
            "close:a",
        ]
    );
}

#[tokio::test]
async fn unready_or_ambiguous_chrome_never_navigates_and_always_closes_sessions() {
    for (discovery, attach, expected_kind, expected_message, expected_attempts) in [
        (
            Discovery::Missing,
            false,
            mcp::ErrorKind::Timeout,
            "resource a discovery timed out",
            None,
        ),
        (
            Discovery::StalledAfterEmpty,
            false,
            mcp::ErrorKind::Timeout,
            "resource a discovery timed out",
            Some(2),
        ),
        (
            Discovery::Missing,
            true,
            mcp::ErrorKind::Setup,
            "resource a matched 0 available devices",
            Some(1),
        ),
        (
            Discovery::Ambiguous,
            false,
            mcp::ErrorKind::Setup,
            "resource a matched 2 available devices",
            Some(1),
        ),
        (
            Discovery::ToolFailure,
            false,
            mcp::ErrorKind::Execution,
            "resource a discovery request failed",
            Some(1),
        ),
    ] {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let mut project = fixture.load();
        project.plan.resources[0].timeout_ms = if matches!(discovery, Discovery::StalledAfterEmpty)
        {
            1000
        } else {
            250
        };
        if attach {
            project.plan.resources[0].prepare = Preparation::Attach {};
        }
        let factory = Factory {
            discovery_a: discovery,
            ..Default::default()
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run(
                &project,
                &factory,
                &fixture.root.join("out"),
                &[],
                Duration::from_secs(5),
            ),
        )
        .await
        .expect("resource discovery must be bounded");
        let Err(error) = result else {
            panic!("discovery must fail")
        };
        assert_eq!(error.kind(), expected_kind, "{error}");
        assert!(error.to_string().contains(expected_message), "{error}");
        assert!(!error.to_string().contains("private-discovery-detail"));
        let events = factory.events.lock().unwrap();
        let count = |event: &str| events.iter().filter(|value| *value == event).count();
        if let Some(expected_attempts) = expected_attempts {
            assert_eq!(count("mobile_list_available_devices:a:"), expected_attempts);
        }
        assert_eq!(
            count("mobile_boot_device:a:chrome:browser"),
            usize::from(!attach)
        );
        assert!(
            !events
                .iter()
                .any(|event| event.starts_with("mobile_open_url")
                    || event.starts_with("mobile_click"))
        );
        assert_eq!(count("close:a"), 1);
        assert_eq!(count("close:b"), 1);
    }
}

#[tokio::test]
async fn prepares_all_owned_sessions_then_routes_each_case_to_its_own_native_id() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - select_device: '${device}'\n  - tap: {x: 1, y: 2}\n",
        "version: 1\ntargets:\n  first: {device: {id: native-first}}\n  second: {device: {id: native-second}}\n",
    );
    let factory = Factory::default();
    let result = run(
        &fixture.load(),
        &factory,
        &fixture.root.join("out"),
        &[],
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(result.reports.len(), 2);
    assert!(result.reports.iter().all(|report| report.status == "PASS"));
    let events = factory.events.lock().unwrap();
    let first_tap = events
        .iter()
        .position(|v| v.starts_with("mobile_click"))
        .unwrap();
    assert!(
        events[..first_tap]
            .iter()
            .any(|v| v == "mobile_open_url:a:chrome:same")
    );
    assert!(
        events[..first_tap]
            .iter()
            .any(|v| v == "mobile_open_url:b:chrome:same")
    );
    assert!(
        events
            .iter()
            .any(|v| v == "mobile_click_on_screen_at_coordinates:a:chrome:same")
    );
    assert!(
        events
            .iter()
            .any(|v| v == "mobile_click_on_screen_at_coordinates:b:chrome:same")
    );
    assert!(events.iter().any(|v| v == "close:a"));
    assert!(events.iter().any(|v| v == "close:b"));
    let last_case_action = events
        .iter()
        .rposition(|v| v.starts_with("mobile_click_on_screen_at_coordinates:"))
        .unwrap();
    let first_close = events.iter().position(|v| v.starts_with("close:")).unwrap();
    assert!(
        first_close > last_case_action,
        "project-owned sessions must stay open through every target case"
    );
    assert_eq!(events.iter().filter(|v| *v == "close:a").count(), 1);
    assert_eq!(events.iter().filter(|v| *v == "close:b").count(), 1);
}
#[tokio::test]
async fn missing_tool_and_profile_fail_before_launch_and_close_owned_connections() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        missing_tool: true,
        ..Factory::default()
    };
    assert!(
        run(
            &fixture.load(),
            &factory,
            &fixture.root.join("out"),
            &[],
            Duration::from_secs(5)
        )
        .await
        .is_err()
    );
    {
        let events = factory.events.lock().unwrap();
        assert!(!events.iter().any(|event| event.starts_with("mobile_boot")));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("close:"))
                .count(),
            2
        );
    }
    fs::write(
        fixture.root.join("profiles.yaml"),
        "version: 1\ntargets: {first: {}}\n",
    )
    .unwrap();
    let factory = Factory::default();
    assert!(
        run(
            &fixture.load(),
            &factory,
            &fixture.root.join("out"),
            &[],
            Duration::from_secs(5)
        )
        .await
        .is_err()
    );
    assert!(factory.events.lock().unwrap().is_empty());
}

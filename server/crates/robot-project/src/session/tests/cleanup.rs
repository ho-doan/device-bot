use super::*;

#[tokio::test]
async fn explicit_project_cancellation_closes_owned_sessions() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        hang_tap: true,
        ..Factory::default()
    };
    let owned_factory = factory.clone();
    let project = fixture.load();
    let output = fixture.root.join("cancel-out");
    let (cancel, receiver) = watch::channel(false);
    let task = tokio::spawn(async move {
        run_with_history_cancel(
            &project,
            &owned_factory,
            &output,
            &[],
            Duration::from_secs(10),
            None,
            receiver,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if factory
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event.starts_with("mobile_click"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    cancel.send(true).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    let error = match result {
        Ok(_) => panic!("project cancellation should interrupt the run"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), robot_flow::outcome::ErrorKind::Interrupted);
    assert_eq!(
        factory
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("close:"))
            .count(),
        2
    );
}

#[tokio::test]
async fn setup_failure_closes_every_owned_session() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        fail_boot: true,
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
    let events = factory.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("close:"))
            .count(),
        2
    );
    assert!(!events.iter().any(|event| event.starts_with("mobile_click")));
}
#[tokio::test]
async fn deadline_and_dropped_project_future_close_owned_sessions() {
    let fixture = Fixture::new(
        "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
        PROFILES,
    );
    let factory = Factory {
        hang_tap: true,
        ..Factory::default()
    };
    let result = run(
        &fixture.load(),
        &factory,
        &fixture.root.join("out"),
        &[],
        Duration::from_millis(30),
    )
    .await
    .unwrap();
    assert_eq!(result.reports[0].error_kind.as_deref(), Some("timeout"));
    assert_eq!(
        factory
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("close:"))
            .count(),
        2
    );
    let factory = Factory {
        hang_tap: true,
        ..Factory::default()
    };
    let owned_factory = factory.clone();
    let project = fixture.load();
    let output = fixture.root.join("other-out");
    let task = tokio::spawn(async move {
        run(
            &project,
            &owned_factory,
            &output,
            &[],
            Duration::from_secs(10),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if factory
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event.starts_with("mobile_click"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if factory
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.starts_with("close:"))
                .count()
                == 2
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

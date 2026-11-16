use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct BrowserState {
    contexts: BTreeMap<String, (u64, String)>,
    next_context: u64,
    connections: u64,
    methods: Vec<String>,
    mouse_events: Vec<Value>,
    hold_mouse_press_until_release: bool,
    reject_mouse_release: bool,
    close_observer: bool,
    observer_events: Option<tokio::sync::mpsc::UnboundedSender<Value>>,
    pause_action: bool,
    action_started: Arc<tokio::sync::Notify>,
    release_action: Arc<tokio::sync::Notify>,
}
struct Fixture {
    robot: ChromeRobot,
    state: Arc<std::sync::Mutex<BrowserState>>,
    tasks: Vec<JoinHandle<()>>,
    artifacts: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.artifacts);
    }
}
async fn fixture() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}/devtools/browser/shared",
        listener.local_addr().unwrap()
    );
    let state = Arc::new(std::sync::Mutex::new(BrowserState::default()));
    let shared = Arc::clone(&state);
    let ws = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let state = Arc::clone(&shared);
            tokio::spawn(async move {
                let owner = {
                    let mut s = state.lock().unwrap();
                    s.connections += 1;
                    s.connections
                };
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let mut held_press = None;
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
                loop {
                    let frame = tokio::select! {
                        frame = socket.next() => match frame { Some(Ok(frame)) => frame, _ => break },
                        event = event_rx.recv() => {
                            if let Some(event) = event {
                                if socket.send(Message::Text(event.to_string().into())).await.is_err() { break; }
                            }
                            continue;
                        }
                    };
                    if frame.is_close() {
                        break;
                    }
                    if !frame.is_text() {
                        continue;
                    }
                    let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                    if request["method"] == "Network.enable" {
                        state.lock().unwrap().observer_events = Some(event_tx.clone());
                    }
                    let gate = {
                        let s = state.lock().unwrap();
                        if s.pause_action && request["method"] == "Page.bringToFront" {
                            Some((s.action_started.clone(), s.release_action.clone()))
                        } else {
                            None
                        }
                    };
                    if let Some((started, release)) = gate {
                        started.notify_one();
                        release.notified().await;
                    }
                    let result = {
                        let mut s = state.lock().unwrap();
                        let method = request["method"].as_str().unwrap();
                        s.methods.push(method.into());
                        match method {
                            "Target.createBrowserContext" => {
                                assert_eq!(request["params"]["disposeOnDetach"], true);
                                s.next_context += 1;
                                let context = format!("context-{}", s.next_context);
                                s.contexts
                                    .insert(context.clone(), (owner, "about:blank".into()));
                                json!({"browserContextId":context})
                            }
                            "Target.createTarget" => {
                                assert_eq!(request["params"]["newWindow"], true);
                                let context =
                                    request["params"]["browserContextId"].as_str().unwrap();
                                assert!(s.contexts.contains_key(context));
                                json!({"targetId":format!("tab-{context}")})
                            }
                            "Target.getTargets" => {
                                let mut targets = vec![
                                    json!({"targetId":"personal","type":"page","title":"Private personal tab","url":"https://personal.example/"}),
                                ];
                                for (context, (_, url)) in &s.contexts {
                                    targets.push(json!({"targetId":format!("tab-{context}"),"browserContextId":context,"type":"page","title":"about:blank","url":url}));
                                    targets.push(json!({"targetId":format!("devtools-{context}"),"browserContextId":context,"type":"page","url":"devtools://devtools/"}));
                                }
                                json!({"targetInfos":targets})
                            }
                            "Target.attachToTarget" => {
                                assert_ne!(request["params"]["targetId"], "personal");
                                assert_eq!(request["params"]["flatten"], true);
                                json!({"sessionId":format!("session-{}", request["params"]["targetId"].as_str().unwrap())})
                            }
                            "Target.disposeBrowserContext" => {
                                let context =
                                    request["params"]["browserContextId"].as_str().unwrap();
                                assert!(s.contexts.remove(context).is_some());
                                json!({})
                            }
                            "Page.captureScreenshot" => {
                                assert!(request["sessionId"].is_string());
                                json!({"data":"cG5n"})
                            }
                            "Page.bringToFront" | "Input.dispatchMouseEvent" => {
                                assert!(request["sessionId"].is_string());
                                if method == "Input.dispatchMouseEvent" {
                                    s.mouse_events.push(request["params"].clone());
                                }
                                json!({})
                            }
                            "Runtime.evaluate" => {
                                assert!(request["sessionId"].is_string());
                                json!({"result":{"value":[{"className":"input","elementType":"textField","value":""}]}})
                            }
                            "Runtime.enable" | "Network.enable" => json!({}),
                            other => {
                                panic!("unexpected command {other}; no Browser.close is permitted")
                            }
                        }
                    };
                    let (hold_press, reject_release) = {
                        let s = state.lock().unwrap();
                        (s.hold_mouse_press_until_release, s.reject_mouse_release)
                    };
                    let mouse = request["method"] == "Input.dispatchMouseEvent";
                    if mouse && request["params"]["type"] == "mousePressed" && hold_press {
                        held_press = Some(request["id"].clone());
                        continue;
                    }
                    let response = if mouse
                        && request["params"]["type"] == "mouseReleased"
                        && reject_release
                    {
                        json!({"id":request["id"],"error":{"code":-32000,"message":"release fixture rejected"}})
                    } else {
                        json!({"id":request["id"],"result":result})
                    };
                    if socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if request["method"] == "Network.enable" {
                        let session = request["sessionId"].clone();
                        for event in [
                            json!({"sessionId":session,"method":"Runtime.executionContextCreated","params":{"context":{"id":7,"auxData":{"frameId":"frame-1"}}}}),
                            json!({"sessionId":session,"method":"Runtime.consoleAPICalled","params":{"type":"log","executionContextId":7,"args":[{"value":"hello secret"}]}}),
                            json!({"sessionId":session,"method":"Network.requestWillBeSent","params":{"frameId":"frame-1","requestId":"request-1","request":{"method":"GET","url":"https://user:pw@example.test/path?token=secret"}}}),
                        ] {
                            let _ = socket.send(Message::Text(event.to_string().into())).await;
                        }
                        if state.lock().unwrap().close_observer {
                            break;
                        }
                    }
                    if mouse && request["params"]["type"] == "mouseReleased" {
                        if let Some(id) = held_press.take() {
                            // Native popup input may acknowledge mouseup before
                            // returning the deferred mousedown acknowledgement.
                            let event =
                                json!({"method":"Page.loadEventFired","params":{"timestamp":1}});
                            let _ = socket.send(Message::Text(event.to_string().into())).await;
                            let _ = socket
                                .send(Message::Text(
                                    json!({"id":id,"result":{}}).to_string().into(),
                                ))
                                .await;
                        }
                    }
                }
                state
                    .lock()
                    .unwrap()
                    .contexts
                    .retain(|_, (id, _)| *id != owner);
            });
        }
    });
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut robot = ChromeRobot::new(http_listener.local_addr().unwrap().to_string());
    let http = tokio::spawn(async move {
        loop {
            let (mut stream, _) = http_listener.accept().await.unwrap();
            let mut header = vec![];
            while !header.ends_with(b"\r\n\r\n") {
                header.push(stream.read_u8().await.unwrap());
            }
            let body = json!({"webSocketDebuggerUrl":endpoint}).to_string();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    let artifacts = std::env::temp_dir().join(format!(
        "device-bot-context-test-{}-{}",
        std::process::id(),
        REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    robot.artifact_dir = artifacts.clone();
    Fixture {
        robot,
        state,
        tasks: vec![ws, http],
        artifacts,
    }
}

#[tokio::test]
async fn click_releases_a_native_popup_before_waiting_for_press_acknowledgement() {
    let f = fixture().await;
    f.state.lock().unwrap().hold_mouse_press_until_release = true;
    f.robot.boot_device("chrome:browser").await.unwrap();
    let id = f.robot.list_devices().await.unwrap()[0].id.clone();
    tokio::time::timeout(
        Duration::from_secs(1),
        f.robot.tap(&id, Point { x: 10., y: 20. }),
    )
    .await
    .expect("mouseup must be sent while the native popup holds the mousedown acknowledgement")
    .unwrap();
    let events = f.state.lock().unwrap().mouse_events.clone();
    assert_eq!(
        events
            .iter()
            .map(|event| event["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["mouseMoved", "mousePressed", "mouseReleased"]
    );
    assert_eq!(events[1]["buttons"], 1);
    assert_eq!(events[2]["buttons"], 0);
    let connection = f.robot.browser_connection.lock().await.clone().unwrap();
    let browser_events = connection
        .lock()
        .await
        .collect_events(Duration::from_millis(1))
        .await
        .unwrap();
    assert!(
        browser_events
            .iter()
            .any(|event| event["method"] == "Page.loadEventFired")
    );
    assert_eq!(f.robot.take_screenshot(&id).await.unwrap(), b"png");
    f.robot.close_session().await.unwrap();
}

#[tokio::test]
async fn rapid_taps_release_each_popup_press_without_timeout() {
    let f = fixture().await;
    f.state.lock().unwrap().hold_mouse_press_until_release = true;
    f.robot.boot_device("chrome:browser").await.unwrap();
    let id = f.robot.list_devices().await.unwrap()[0].id.clone();
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::try_join!(
            f.robot.tap(&id, Point { x: 10., y: 20. }),
            f.robot.tap(&id, Point { x: 10., y: 20. }),
        )
    })
    .await
    .expect("rapid taps must release both presses before their acknowledgements")
    .unwrap();
    let events = f.state.lock().unwrap().mouse_events.clone();
    let buttons: Vec<_> = events
        .iter()
        .filter(|event| event["type"] != "mouseMoved")
        .collect();
    assert_eq!(
        buttons
            .iter()
            .map(|event| event["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "mousePressed",
            "mouseReleased",
            "mousePressed",
            "mouseReleased"
        ]
    );
    assert_eq!(
        buttons
            .iter()
            .map(|event| event["buttons"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 0, 1, 0]
    );
    assert_eq!(f.robot.take_screenshot(&id).await.unwrap(), b"png");
    f.robot.close_session().await.unwrap();
}

#[tokio::test]
async fn pipelined_click_still_reports_mouse_release_errors() {
    let f = fixture().await;
    {
        let mut state = f.state.lock().unwrap();
        state.hold_mouse_press_until_release = true;
        state.reject_mouse_release = true;
    }
    f.robot.boot_device("chrome:browser").await.unwrap();
    let id = f.robot.list_devices().await.unwrap()[0].id.clone();
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        f.robot.tap(&id, Point { x: 10., y: 20. }),
    )
    .await
    .expect("mouseup must not wait for the held mousedown acknowledgement")
    .unwrap_err();
    assert!(
        error.to_string().contains("release fixture rejected"),
        "{error}"
    );
    assert_eq!(f.robot.take_screenshot(&id).await.unwrap(), b"png");
    f.robot.close_session().await.unwrap();
}

#[tokio::test]
async fn clients_share_browser_connection_but_own_and_dispose_only_their_contexts() {
    let f = fixture().await;
    let second = f.robot.fork_session().unwrap();
    f.robot.boot_device("chrome:browser").await.unwrap();
    second.boot_device("chrome:browser").await.unwrap();
    let first_page = f.robot.list_devices().await.unwrap();
    let second_page = second.list_devices().await.unwrap();
    assert_eq!(first_page.len(), 1);
    assert_eq!(second_page.len(), 1);
    assert_ne!(first_page[0].id, second_page[0].id);
    assert_eq!(
        f.state.lock().unwrap().connections,
        1,
        "reuse the authorized browser connection"
    );
    assert_eq!(
        f.robot.take_screenshot(&first_page[0].id).await.unwrap(),
        b"png"
    );
    assert!(f.robot.take_screenshot("chrome:personal").await.is_err());
    assert!(f.robot.take_screenshot(&second_page[0].id).await.is_err());
    f.robot.close_session().await.unwrap();
    assert_eq!(f.state.lock().unwrap().contexts.len(), 1);
    assert_eq!(
        second.list_devices().await.unwrap()[0].id,
        second_page[0].id
    );
    assert_eq!(
        second.take_screenshot(&second_page[0].id).await.unwrap(),
        b"png"
    );
    second.close_session().await.unwrap();
    assert!(f.state.lock().unwrap().contexts.is_empty());
    assert!(
        !f.state
            .lock()
            .unwrap()
            .methods
            .iter()
            .any(|m| m == "Browser.close")
    );
}

#[tokio::test]
async fn concurrent_clients_boot_distinct_contexts_on_one_shared_browser_connection() {
    let f = fixture().await;
    let second = f.robot.fork_session().unwrap();
    let (first_boot, second_boot) = tokio::join!(
        f.robot.boot_device("chrome:browser"),
        second.boot_device("chrome:browser")
    );
    first_boot.unwrap();
    second_boot.unwrap();
    let first = f.robot.list_devices().await.unwrap()[0].id.clone();
    let other = second.list_devices().await.unwrap()[0].id.clone();
    assert_ne!(first, other);
    assert_eq!(f.state.lock().unwrap().connections, 1);
    assert_eq!(f.state.lock().unwrap().contexts.len(), 2);
    f.robot.close_session().await.unwrap();
    assert_eq!(second.take_screenshot(&other).await.unwrap(), b"png");
    second.close_session().await.unwrap();
    assert!(f.state.lock().unwrap().contexts.is_empty());
}

#[tokio::test]
async fn closing_after_failed_action_leaves_other_client_usable_and_next_case_gets_new_context() {
    let f = fixture().await;
    let next = f.robot.fork_session().unwrap();
    f.robot.boot_device("chrome:browser").await.unwrap();
    let old_page = f.robot.list_devices().await.unwrap()[0].id.clone();
    assert!(
        f.robot
            .tap("chrome:not-owned", Point { x: 1.0, y: 1.0 })
            .await
            .is_err()
    );
    f.robot.close_session().await.unwrap();
    next.boot_device("chrome:browser").await.unwrap();
    let new_page = next.list_devices().await.unwrap()[0].id.clone();
    assert_ne!(old_page, new_page);
    assert_eq!(f.state.lock().unwrap().connections, 1);
    next.close_session().await.unwrap();
    assert!(f.state.lock().unwrap().contexts.is_empty());
}

#[tokio::test]
async fn dropped_client_disposes_context_without_closing_shared_browser() {
    let f = fixture().await;
    let client = f.robot.fork_session().unwrap();
    client.boot_device("chrome:browser").await.unwrap();
    drop(client);
    for _ in 0..100 {
        if f.state.lock().unwrap().contexts.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(f.state.lock().unwrap().contexts.is_empty());
    f.robot.boot_device("chrome:browser").await.unwrap();
    assert_eq!(f.state.lock().unwrap().connections, 1);
    f.robot.close_session().await.unwrap();
}

#[tokio::test]
async fn restart_replaces_only_the_requesting_case_context() {
    let fixture = fixture().await;
    let first = fixture.robot.fork_session().unwrap();
    let second = fixture.robot.fork_session().unwrap();
    first.boot_device("chrome:browser").await.unwrap();
    second.boot_device("chrome:browser").await.unwrap();
    let second_id = second.list_devices().await.unwrap()[0].id.clone();
    let old_id = first.list_devices().await.unwrap()[0].id.clone();
    first.restart_device("chrome:browser").await.unwrap();
    assert_ne!(first.list_devices().await.unwrap()[0].id, old_id);
    assert_eq!(second.list_devices().await.unwrap()[0].id, second_id);
    assert_eq!(fixture.state.lock().unwrap().contexts.len(), 2);
    first.close_session().await.unwrap();
    second.close_session().await.unwrap();
}

#[tokio::test]
async fn observation_reads_its_own_socket_and_cleans_up() {
    let f = fixture().await;
    f.robot.boot_device("chrome:browser").await.unwrap();
    let id = f.robot.list_devices().await.unwrap()[0].id.clone();
    let observer = f
        .robot
        .start_observation(
            &id,
            robot_core::ObservationOptions {
                include_text: true,
                redact: vec!["secret".into()],
            },
        )
        .await
        .unwrap();
    f.robot.tap(&id, Point { x: 1., y: 2. }).await.unwrap();
    let batch = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let batch = f
                .robot
                .poll_observation(&id, &observer, 0, 100)
                .await
                .unwrap();
            if batch.records.len() >= 2 {
                break batch;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(batch.records[0].frame_id.as_deref(), Some("frame-1"));
    let serialized = serde_json::to_string(&batch).unwrap();
    assert!(serialized.contains("[redacted]"));
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("user:pw"));
    assert!(serialized.contains("https://example.test/path"));
    assert!(
        f.robot
            .poll_observation("chrome:other", &observer, 0, 10)
            .await
            .is_err()
    );
    f.robot.stop_observation(&id, &observer).await.unwrap();
    assert!(
        f.robot
            .poll_observation(&id, &observer, 0, 10)
            .await
            .is_err()
    );
    f.robot.close_session().await.unwrap();
}

#[tokio::test]
async fn observation_disconnect_is_reported_and_sessions_are_isolated() {
    let f = fixture().await;
    let second = f.robot.fork_session().unwrap();
    f.robot.boot_device("chrome:browser").await.unwrap();
    second.boot_device("chrome:browser").await.unwrap();
    let first_id = f.robot.list_devices().await.unwrap()[0].id.clone();
    let second_id = second.list_devices().await.unwrap()[0].id.clone();
    f.state.lock().unwrap().close_observer = true;
    let obs = f
        .robot
        .start_observation(&first_id, Default::default())
        .await
        .unwrap();
    assert!(
        second
            .poll_observation(&first_id, &obs, 0, 1)
            .await
            .is_err()
    );
    assert!(
        f.robot
            .poll_observation(&second_id, &obs, 0, 1)
            .await
            .is_err()
    );
    let batch = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let batch = f
                .robot
                .poll_observation(&first_id, &obs, 0, 10)
                .await
                .unwrap();
            if batch.closed {
                break batch;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(batch.terminal_reason.is_some());
    f.robot.close_session().await.unwrap();
    assert!(
        f.robot
            .poll_observation(&first_id, &obs, 0, 1)
            .await
            .is_err()
    );
    assert!(!second.list_devices().await.unwrap().is_empty());
    second.close_session().await.unwrap();
}

#[tokio::test]
async fn observer_receives_post_start_events_during_action_and_idle() {
    let f = fixture().await;
    f.robot.boot_device("chrome:browser").await.unwrap();
    let id = f.robot.list_devices().await.unwrap()[0].id.clone();
    let observer = f
        .robot
        .start_observation(
            &id,
            robot_core::ObservationOptions {
                include_text: true,
                redact: vec![],
            },
        )
        .await
        .unwrap();
    let (sender, started, release) = {
        let mut state = f.state.lock().unwrap();
        state.pause_action = true;
        (
            state.observer_events.clone().unwrap(),
            state.action_started.clone(),
            state.release_action.clone(),
        )
    };
    let actor = f.robot.clone();
    let action_id = id.clone();
    let action = tokio::spawn(async move { actor.tap(&action_id, Point { x: 1., y: 2. }).await });
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    let session = format!("session-{}", id.strip_prefix("chrome:").unwrap());
    let emit = |marker: &str| {
        sender.send(json!({"sessionId":session,"method":"Runtime.consoleAPICalled","params":{"type":"log","executionContextId":7,"args":[{"value":marker}]}})).unwrap()
    };
    emit("during-action-marker");
    let robot = &f.robot;
    let id_ref = &id;
    let observer_ref = &observer;
    let wait_for = |marker: &'static str| async move {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let batch = robot
                    .poll_observation(id_ref, observer_ref, 0, 100)
                    .await
                    .unwrap();
                if serde_json::to_string(&batch).unwrap().contains(marker) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    };
    wait_for("during-action-marker").await;
    assert!(!action.is_finished());
    release.notify_one();
    action.await.unwrap().unwrap();
    emit("idle-marker");
    wait_for("idle-marker").await;
    f.robot.stop_observation(&id, &observer).await.unwrap();
    f.robot.close_session().await.unwrap();
}

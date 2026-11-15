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
                while let Some(Ok(frame)) = socket.next().await {
                    if frame.is_close() {
                        break;
                    }
                    if !frame.is_text() {
                        continue;
                    }
                    let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
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

use super::*;
use robot_core::{ObservationBatch, ObservationOptions, ObservationRecord};
use std::collections::VecDeque;
use tokio::time::Instant;

const CAPACITY: usize = 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);

pub(super) struct Observer {
    page: String,
    state: Arc<Mutex<State>>,
    task: Option<JoinHandle<()>>,
}
impl Drop for Observer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
#[derive(Default)]
struct State {
    records: VecDeque<ObservationRecord>,
    sequence: u64,
    dropped: u64,
    closed: bool,
    terminal_reason: Option<String>,
}

fn redact_values(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(s) => *s = clean_text(s, secrets),
        Value::Array(items) => {
            for item in items {
                redact_values(item, secrets);
            }
        }
        Value::Object(items) => {
            for item in items.values_mut() {
                redact_values(item, secrets);
            }
        }
        _ => {}
    }
}

fn clean_text(input: &str, secrets: &[String]) -> String {
    let mut value = input.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        value = value.replace(secret, "[redacted]");
    }
    value.chars().take(512).collect()
}
fn clean_url(input: &str) -> String {
    if !["http://", "https://", "ws://", "wss://"]
        .iter()
        .any(|scheme| input.starts_with(scheme))
    {
        return String::new();
    }
    let url = input.split(['?', '#']).next().unwrap_or("");
    let url = if let Some((scheme, tail)) = url.split_once("://") {
        if let Some((authority, path)) = tail.split_once('/') {
            format!(
                "{}://{}/{}",
                scheme,
                authority.rsplit('@').next().unwrap_or(""),
                path
            )
        } else {
            format!("{}://{}", scheme, tail.rsplit('@').next().unwrap_or(""))
        }
    } else {
        url.to_owned()
    };
    url.chars().take(512).collect()
}
fn event_data(event: &Value, options: &ObservationOptions) -> Option<(String, Value)> {
    let method = event["method"].as_str()?;
    let p = &event["params"];
    let data = match method {
        "Runtime.consoleAPICalled" => {
            let mut d = json!({"level":p["type"],"executionContextId":p["executionContextId"],"argumentCount":p["args"].as_array().map_or(0, Vec::len),"url":clean_url(p["stackTrace"]["callFrames"][0]["url"].as_str().unwrap_or(""))});
            if options.include_text {
                let text = p["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["value"].as_str())
                    .map(|s| clean_text(s, &options.redact))
                    .collect::<Vec<_>>();
                d["text"] = json!(text.into_iter().take(16).collect::<Vec<_>>());
            }
            d
        }
        "Runtime.exceptionThrown" => {
            let details = &p["exceptionDetails"];
            let mut d = json!({"executionContextId":details["executionContextId"],"line":details["lineNumber"],"column":details["columnNumber"],"url":clean_url(details["url"].as_str().unwrap_or(""))});
            if options.include_text {
                d["text"] = json!(clean_text(
                    details["exception"]["description"]
                        .as_str()
                        .or_else(|| details["text"].as_str())
                        .unwrap_or(""),
                    &options.redact
                ));
            }
            d
        }
        "Network.requestWillBeSent" => {
            json!({"requestId":p["requestId"],"method":p["request"]["method"],"url":clean_url(p["request"]["url"].as_str().unwrap_or("")),"resourceType":p["type"],"redirectStatus":p["redirectResponse"]["status"]})
        }
        "Network.responseReceived" => {
            json!({"requestId":p["requestId"],"status":p["response"]["status"],"url":clean_url(p["response"]["url"].as_str().unwrap_or("")),"resourceType":p["type"],"mimeType":p["response"]["mimeType"]})
        }
        "Network.loadingFailed" => {
            json!({"requestId":p["requestId"],"canceled":p["canceled"],"blockedReason":p["blockedReason"]})
        }
        "Network.webSocketFrameSent" | "Network.webSocketFrameReceived" => {
            json!({"requestId":p["requestId"],"opcode":p["response"]["opcode"],"payloadEncodedBytes":p["response"]["payloadData"].as_str().map_or(0, str::len)})
        }
        "Network.eventSourceMessageReceived" => {
            json!({"requestId":p["requestId"],"dataBytes":p["data"].as_str().map_or(0, str::len)})
        }
        _ => return None,
    };
    let mut data = data;
    redact_values(&mut data, &options.redact);
    Some((method.to_owned(), data))
}
async fn record(
    state: &Mutex<State>,
    event: Value,
    page: &str,
    contexts: &mut HashMap<i64, String>,
    options: &ObservationOptions,
    started: Instant,
) {
    let p = &event["params"];
    if event["method"] == "Runtime.executionContextCreated" {
        if let (Some(id), Some(frame)) = (
            p["context"]["id"].as_i64(),
            p["context"]["auxData"]["frameId"].as_str(),
        ) {
            contexts.insert(id, frame.to_owned());
        }
    } else if event["method"] == "Runtime.executionContextDestroyed" {
        if let Some(id) = p["executionContextId"].as_i64() {
            contexts.remove(&id);
        }
    } else if event["method"] == "Runtime.executionContextsCleared" {
        contexts.clear();
    }
    let Some((kind, data)) = event_data(&event, options) else {
        return;
    };
    let frame = p["frameId"].as_str().map(str::to_owned).or_else(|| {
        data["executionContextId"]
            .as_i64()
            .and_then(|id| contexts.get(&id).cloned())
    });
    let mut state = state.lock().await;
    state.sequence += 1;
    let item = ObservationRecord {
        sequence: state.sequence,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        page_id: page.to_owned(),
        frame_id: frame,
        kind,
        data,
    };
    if state.records.len() == CAPACITY {
        state.records.pop_front();
        state.dropped += 1;
    }
    state.records.push_back(item);
}
impl ChromeRobot {
    pub(super) async fn begin_observer(
        &self,
        device: &str,
        mut options: ObservationOptions,
    ) -> Result<String, RobotError> {
        if options.redact.len() > 64 || options.redact.iter().any(|s| s.len() > 1024) {
            return Err(RobotError::InvalidInput(
                "redaction list exceeds limits".into(),
            ));
        }
        options.redact.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let page = self.page(device).await?;
        let endpoint = self.running_browser_endpoint().await?;
        let mut c = connection::BrowserConnection::connect(&endpoint).await?;
        let attached = c
            .request(
                "Target.attachToTarget",
                json!({"targetId":page.id,"flatten":true}),
                None,
            )
            .await?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| RobotError::CommandFailed("missing observer session".into()))?
            .to_owned();
        c.request("Runtime.enable", json!({}), Some(session.clone()))
            .await?;
        c.request("Network.enable", json!({}), Some(session.clone()))
            .await?;
        let buffered = c.collect_events(Duration::ZERO).await?;
        let state = Arc::new(Mutex::new(State {
            dropped: c.dropped_events(),
            ..State::default()
        }));
        let task_state = state.clone();
        let page_id = page.id.clone();
        let started = Instant::now();
        let task = tokio::spawn(async move {
            let mut contexts = HashMap::new();
            for event in buffered {
                if event["sessionId"] == session {
                    record(
                        &task_state,
                        event,
                        &page_id,
                        &mut contexts,
                        &options,
                        started,
                    )
                    .await;
                }
            }
            let mut reason = "connection closed";
            while let Some(result) = c.socket.next().await {
                let frame = match result {
                    Ok(frame) => frame,
                    Err(_) => {
                        reason = "transport error";
                        break;
                    }
                };
                if frame.is_close() {
                    break;
                }
                if let Ok(text) = frame.to_text() {
                    if let Ok(event) = serde_json::from_str::<Value>(text) {
                        if event["sessionId"] == session {
                            record(
                                &task_state,
                                event,
                                &page_id,
                                &mut contexts,
                                &options,
                                started,
                            )
                            .await;
                        }
                    }
                }
            }
            let mut state = task_state.lock().await;
            state.closed = true;
            state.terminal_reason = Some(reason.into());
        });
        let id = format!("obs-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let mut observers = self.observers.lock().await;
        if observers.len() >= 16 {
            task.abort();
            return Err(RobotError::InvalidInput("observer limit reached".into()));
        }
        observers.insert(
            id.clone(),
            Observer {
                page: device.to_owned(),
                state,
                task: Some(task),
            },
        );
        Ok(id)
    }
    pub(super) async fn read_observer(
        &self,
        device: &str,
        id: &str,
        after: u64,
        limit: usize,
    ) -> Result<ObservationBatch, RobotError> {
        let observers = self.observers.lock().await;
        let observer = observers
            .get(id)
            .filter(|o| o.page == device)
            .ok_or_else(|| RobotError::InvalidInput("unknown observation".into()))?;
        let state = observer.state.lock().await;
        Ok(ObservationBatch { records: state.records.iter().filter(|r| r.sequence > after).take(limit.clamp(1, CAPACITY)).cloned().collect(), dropped: state.dropped, closed: state.closed, terminal_reason: state.terminal_reason.clone(), limitations: vec!["Only the attached page target is captured; out-of-process iframe targets and workers are not captured. Frame IDs may be unavailable for some events. Bodies and headers are excluded.".into()] })
    }
    pub(super) async fn end_observer(&self, device: &str, id: &str) -> Result<(), RobotError> {
        let mut observers = self.observers.lock().await;
        if !observers.get(id).is_some_and(|o| o.page == device) {
            return Err(RobotError::InvalidInput("unknown observation".into()));
        }
        let mut observer = observers.remove(id).unwrap();
        if let Some(task) = observer.task.take() {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
    pub(super) async fn end_all_observers(&self) {
        let observers = std::mem::take(&mut *self.observers.lock().await);
        for (_, mut observer) in observers {
            if let Some(task) = observer.task.take() {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_and_exception_metadata_are_sanitized() {
        let options = ObservationOptions {
            include_text: true,
            redact: vec!["secret".into()],
        };
        for method in [
            "Network.responseReceived",
            "Network.loadingFailed",
            "Network.webSocketFrameSent",
            "Network.webSocketFrameReceived",
            "Network.eventSourceMessageReceived",
            "Runtime.exceptionThrown",
        ] {
            let event = json!({"method":method,"params":{"requestId":"secret","response":{"url":"https://u:p@example.test/secret?q=hidden","payloadData":"secret"},"data":"secret","exceptionDetails":{"text":"secret"}}});
            let (_, data) = event_data(&event, &options).unwrap();
            let serialized = data.to_string();
            assert!(!serialized.contains("secret"));
            assert!(!serialized.contains("hidden"));
            assert!(!serialized.contains("u:p"));
        }
    }
    #[tokio::test]
    async fn overflow_is_counted_and_default_drops_content() {
        let state = Mutex::new(State::default());
        let mut contexts = HashMap::new();
        let options = ObservationOptions::default();
        let start = Instant::now();
        for _ in 0..CAPACITY + 5 {
            record(&state, json!({"method":"Runtime.consoleAPICalled","params":{"type":"log","args":[{"value":"private"}]}}), "page", &mut contexts, &options, start).await;
        }
        let state = state.lock().await;
        assert_eq!(state.records.len(), CAPACITY);
        assert_eq!(state.dropped, 5);
        assert_eq!(state.records[0].sequence, 6);
        assert!(
            !serde_json::to_string(&state.records)
                .unwrap()
                .contains("private")
        );
    }
}

#[cfg(test)]
mod live_smoke {
    use super::*;
    struct ChromeProcess {
        child: std::process::Child,
        profile: PathBuf,
    }
    impl Drop for ChromeProcess {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.profile);
        }
    }
    #[tokio::test]
    #[ignore = "requires DEVICE_BOT_TEST_CHROME_BINARY and launches an isolated headless Chrome"]
    async fn isolated_chrome_observes_console_and_http_after_start() {
        let binary =
            std::env::var("DEVICE_BOT_TEST_CHROME_BINARY").expect("set disposable Chrome binary");
        let profile = std::env::temp_dir().join(format!(
            "device-bot-observer-smoke-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&profile).unwrap();
        let child = std::process::Command::new(binary)
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut chrome = ChromeProcess { child, profile };
        let port = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(text) =
                    tokio::fs::read_to_string(chrome.profile.join("DevToolsActivePort")).await
                {
                    if let Some(port) = text.lines().next().and_then(|v| v.parse::<u16>().ok()) {
                        break port;
                    }
                }
                assert!(
                    chrome.child.try_wait().unwrap().is_none(),
                    "headless Chrome exited"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let fixture = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut request = [0_u8; 4096];
                    let count = stream.read(&mut request).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&request[..count]);
                    let body = if request.starts_with("GET /fixture ") {
                        "<html><script>console.log('observer-live-marker'); fetch('/api?token=private');</script></html>"
                    } else {
                        "ok"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        let mut robot = ChromeRobot::new(format!("127.0.0.1:{port}"));
        robot.artifact_dir = chrome.profile.join("artifacts");
        robot.boot_device("chrome:browser").await.unwrap();
        let id = robot.list_devices().await.unwrap()[0].id.clone();
        let observer = robot
            .start_observation(
                &id,
                ObservationOptions {
                    include_text: true,
                    redact: vec![],
                },
            )
            .await
            .unwrap();
        robot.open_url(&id, &url, None).await.unwrap();
        let batch = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let batch = robot
                    .poll_observation(&id, &observer, 0, 1024)
                    .await
                    .unwrap();
                let console = batch.records.iter().any(|r| {
                    r.kind == "Runtime.consoleAPICalled"
                        && r.data.to_string().contains("observer-live-marker")
                });
                let request = batch.records.iter().any(|r| {
                    r.kind == "Network.requestWillBeSent"
                        && r.data["url"].as_str().is_some_and(|u| u.ends_with("/api"))
                });
                let response = batch.records.iter().any(|r| {
                    r.kind == "Network.responseReceived"
                        && r.data["url"].as_str().is_some_and(|u| u.ends_with("/api"))
                        && r.data["status"] == 200
                });
                if console && request && response {
                    break batch;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(batch.dropped, 0);
        assert!(!serde_json::to_string(&batch).unwrap().contains("private"));
        robot.stop_observation(&id, &observer).await.unwrap();
        robot.close_session().await.unwrap();
        fixture.abort();
    }
}

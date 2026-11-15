//! A committed action can lose its HTTP reply. Never replay that action blindly.
use robot_flow::{FlowDocument, FlowRunner, Profile, http_mcp::HttpMcp};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn lost_action_reply_stops_at_step_five_without_retry_and_preserves_cleanup_and_evidence() {
    let temp = Temp(std::env::temp_dir().join(format!(
            "lost-mcp-reply-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&temp.0).unwrap();
    let recording = temp.0.join("capture.mp4");
    std::fs::write(&recording, b"video fixture").unwrap();
    let actions = Arc::new(Mutex::new(Vec::<Value>::new()));
    let observed = actions.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&bytes[..end])
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break (end + 4, length);
                    }
                }
            };
            let closing = bytes.starts_with(b"DELETE ");
            let reply = if length == 0 {
                Value::Null
            } else {
                let request: Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "initialize" => json!({"protocolVersion":"2025-11-25"}),
                    "notifications/initialized" => Value::Null,
                    "tools/list" => {
                        json!({"tools":(["mobile_list_available_devices", "mobile_start_screen_recording",
                        "mobile_stop_screen_recording", "mobile_type_keys", "mobile_take_screenshot", "mobile_click_on_screen_at_coordinates"]
                        .map(|name| json!({"name":name,"inputSchema":{}})))})
                    }
                    "tools/call" => {
                        let params = &request["params"];
                        observed.lock().unwrap().push(params.clone());
                        match params["name"].as_str().unwrap() {
                            "mobile_list_available_devices" => {
                                json!({"structuredContent":{"devices":[{"id":"fixture","platform":"chrome"}]}})
                            }
                            "mobile_take_screenshot" => {
                                json!({"content":[{"type":"image","mimeType":"image/png","data":"iVBORw0KGgo="}]})
                            }
                            "mobile_stop_screen_recording" => {
                                json!({"content":[{"type":"text","text":format!("Screen recording saved: {}", recording.display())}]})
                            }
                            // Commit the tap above, then lose the reply as a broken connection would.
                            "mobile_click_on_screen_at_coordinates" => continue,
                            "mobile_start_screen_recording" | "mobile_type_keys" => {
                                json!({"content":[]})
                            }
                            other => panic!("unexpected tool {other}"),
                        }
                    }
                    other => panic!("unexpected method {other}"),
                };
                if request.get("id").is_some() {
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                } else {
                    Value::Null
                }
            };
            let body = if reply.is_null() {
                String::new()
            } else {
                reply.to_string()
            };
            let status = if body.is_empty() {
                "204 No Content"
            } else {
                "200 OK"
            };
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nmcp-session-id: fixture-session\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            if closing {
                break;
            }
        }
    });
    let mut steps = vec![
        json!({"id":"1","select_device":{"platform":"chrome"}}),
        json!({"id":"2","start_screen_recording":{}}),
        json!({"id":"3","type_text":{"text":"secret-fixture-input"}}),
        json!({"id":"4","screenshot":{"path":"before-failure.png"}}),
        json!({"id":"5","tap":{"x":1,"y":2}}),
    ];
    steps.extend(
        (6..=20).map(|id| json!({"id":id.to_string(),"type_text":{"text":"MUST NOT RUN"}})),
    );
    let yaml =
        serde_yaml::to_string(&json!({"version":2,"name":"lost reply","steps":steps})).unwrap();
    let client = HttpMcp::new(&format!("http://{address}/mcp"), Duration::from_secs(2)).unwrap();
    let evidence = temp.0.join("evidence");
    let report = FlowRunner::new(Some(Box::new(client)), evidence.clone())
        .run(
            &FlowDocument::from_yaml(&yaml).unwrap(),
            &Profile::default(),
            &[],
            Duration::from_secs(5),
        )
        .await;
    server.await.unwrap();
    assert_eq!(report.status, "FAIL");
    assert_eq!(report.error_kind.as_deref(), Some("execution"));
    assert_eq!(report.steps.len(), 5);
    assert_eq!(report.steps[4].id, "5");
    assert_eq!(report.steps[4].status, "failed");
    let actions = actions.lock().unwrap();
    assert_eq!(
        actions
            .iter()
            .filter(|p| p["name"] == "mobile_click_on_screen_at_coordinates")
            .count(),
        1
    );
    assert_eq!(
        actions
            .iter()
            .filter(|p| p["name"] == "mobile_type_keys")
            .count(),
        1
    );
    assert!(
        actions
            .iter()
            .any(|p| p["name"] == "mobile_stop_screen_recording")
    );
    assert!(evidence.join("before-failure.png").is_file());
    assert_eq!(
        std::fs::read(evidence.join("video.mp4")).unwrap(),
        b"video fixture"
    );
    for name in ["mcp-calls.jsonl", "expanded-flow.yaml"] {
        let text = std::fs::read_to_string(evidence.join(name)).unwrap();
        assert!(
            !text.contains("secret-fixture-input"),
            "secret leaked in {name}"
        );
    }
}

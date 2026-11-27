use super::*;

async fn serve_http(response: Vec<u8>, keep_open: bool) -> (ChromeRobot, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let robot = ChromeRobot::new(listener.local_addr().unwrap().to_string());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![];
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        // Fragment both headers and body to exercise TCP framing.
        for chunk in response.chunks(7) {
            stream.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
        if keep_open {
            std::future::pending::<()>().await;
        }
    });
    (robot, task)
}

#[tokio::test]
async fn discovery_finishes_when_body_arrives_before_connection_closes() {
    let body = br#"[{"id":"A1B2","type":"page","title":"fixture","url":"about:blank","webSocketDebuggerUrl":"ws://localhost/page/A1B2"}]"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncOnTeNt-LeNgTh: {}\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    );
    let (robot, server) = serve_http(response.into_bytes(), true).await;
    let result = tokio::time::timeout(Duration::from_secs(1), robot.endpoint_targets()).await;
    server.abort();
    let targets = result.expect("discovery waited for EOF").unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(device_id(&targets[0]), "chrome:A1B2");
}

#[tokio::test]
async fn http_rejects_truncated_body() {
    let (robot, server) = serve_http(
        b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n[]".to_vec(),
        false,
    )
    .await;
    assert!(robot.http_get("/json/list").await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn http_without_content_length_reads_until_close() {
    let (robot, server) = serve_http(b"HTTP/1.0 200 OK\r\n\r\n[]".to_vec(), false).await;
    assert_eq!(robot.http_get("/json/list").await.unwrap(), b"[]");
    server.await.unwrap();
}

#[tokio::test]
async fn list_elements_preserves_empty_field_value() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let body = json!([{"id":"A1B2","type":"page","webSocketDebuggerUrl":endpoint}]).to_string();
    let (robot, http) = serve_http(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
        false,
    )
    .await;
    let ws = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let request: Value =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        assert_eq!(request["method"], "Runtime.evaluate");
        assert_eq!(request["params"]["awaitPromise"], true);
        socket
            .send(Message::Text(
                json!({"id":request["id"],"result":{"result":{"value":[
                    {"className":"input","value":"","elementType":"textField"},
                    {"className":"input","value":"hello"},
                    {"className":"div","value":null}
                ]}}})
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
    });
    let target = robot.endpoint_targets().await.unwrap().remove(0);
    let elements = robot.query_dom(&target).await.unwrap();
    assert_eq!(elements[0].value.as_deref(), Some(""));
    assert_eq!(elements[1].value.as_deref(), Some("hello"));
    assert_eq!(elements[2].value, None);
    assert_eq!(elements[0].resource_id, None);
    ws.await.unwrap();
    http.await.unwrap();
}

#[tokio::test]
async fn tap_activates_target_before_mouse_input_and_propagates_activation_failure() {
    for activation_fails in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let body = json!([{"id":"A1B2","type":"page","webSocketDebuggerUrl":endpoint}]).to_string();
        let (robot, http) = serve_http(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
            false,
        )
        .await;
        let ws = tokio::spawn(async move {
            let expected = if activation_fails {
                vec!["Page.bringToFront"]
            } else {
                vec![
                    "Page.bringToFront",
                    "Input.dispatchMouseEvent",
                    "Input.dispatchMouseEvent",
                    "Input.dispatchMouseEvent",
                ]
            };
            let mut current_socket = None;
            for (index, method) in expected.iter().enumerate() {
                if index != 3 {
                    let (stream, _) = listener.accept().await.unwrap();
                    current_socket = Some(tokio_tungstenite::accept_async(stream).await.unwrap());
                }
                let socket = current_socket.as_mut().unwrap();
                let request: Value = serde_json::from_str(
                    &socket.next().await.unwrap().unwrap().into_text().unwrap(),
                )
                .unwrap();
                assert_eq!(request["method"], *method);
                if index > 0 {
                    assert_eq!(
                        request["params"]["type"],
                        if index == 1 {
                            "mouseMoved"
                        } else if index == 2 {
                            "mousePressed"
                        } else {
                            "mouseReleased"
                        }
                    );
                    assert_eq!(request["params"]["x"].as_f64(), Some(10.0));
                    assert_eq!(request["params"]["y"].as_f64(), Some(20.0));
                    assert_eq!(request["params"]["buttons"], if index == 2 { 1 } else { 0 });
                }
                let response = if activation_fails {
                    json!({"id":request["id"],"error":{"code":-32000,"message":"activation failed"}})
                } else {
                    json!({"id":request["id"],"result":{}})
                };
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            robot.dispatch_tap(
                &robot.endpoint_targets().await.unwrap().remove(0),
                Point { x: 10.0, y: 20.0 },
            ),
        )
        .await
        .unwrap();
        if activation_fails {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("activation failed")
            );
        } else {
            result.unwrap();
        }
        ws.await.unwrap();
        http.await.unwrap();
    }
}

#[test]
fn chrome_target_becomes_a_device_id() {
    let target = ChromeTarget {
        id: "A1B2".into(),
        title: String::new(),
        url: String::new(),
        kind: "page".into(),
        web_socket_debugger_url: Some("ws://localhost/devtools/page/A1B2".into()),
    };
    assert_eq!(device_id(&target), "chrome:A1B2");
}

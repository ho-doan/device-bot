use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "requires disposable Chrome at DEVICE_BOT_TEST_CHROME_ENDPOINT"]
async fn real_iframe_context_actions_and_navigation_rejection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut bytes = [0u8; 4096];
                let count = stream.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let body = match path {
                    "/" => String::from(r#"<input id="editor"><iframe id="outer" src="/same"></iframe><iframe id="other" src="/other"></iframe>"#),
                    "/same" => format!(r#"<iframe id="inner" src="http://localhost:{port}/cross"></iframe>"#),
                    _ => r#"<input id="editor"><button id="submit" onclick="this.textContent='Paid'">Pay</button><div style="height:1800px"></div>"#.into(),
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
    let endpoint = std::env::var("DEVICE_BOT_TEST_CHROME_ENDPOINT").unwrap();
    let robot = ChromeRobot::new(endpoint);
    robot.boot_device("chrome:browser").await.unwrap();
    let id = robot.list_devices().await.unwrap()[0].id.clone();
    robot
        .open_url(&id, &format!("http://127.0.0.1:{port}/"), None)
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let rows = loop {
        match robot
            .list_elements_query(
                &id,
                ElementQuery {
                    include_frames: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(rows)
                if rows.iter().any(|row| {
                    row.web_context
                        .as_ref()
                        .is_some_and(|ctx| ctx.frame_path.len() == 2)
                }) =>
            {
                break rows;
            }
            result => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "nested OOPIF never became inspectable: {result:?}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    let field = rows
        .iter()
        .find(|row| {
            row.resource_id.as_deref() == Some("editor")
                && row
                    .web_context
                    .as_ref()
                    .is_some_and(|ctx| ctx.frame_path.len() == 2)
        })
        .unwrap();
    let context = field.web_context.clone().unwrap();
    let target = robot.page(&id).await.unwrap();
    let (_, attached) = robot
        .browser_session
        .lock()
        .await
        .as_mut()
        .unwrap()
        .discover_frames(&target)
        .await
        .unwrap();
    assert!(
        !attached.is_empty(),
        "cross-site child must really be an OOPIF"
    );
    robot
        .tap_in_context(&id, Point { x: 0., y: 0. }, Some(&context))
        .await
        .unwrap();
    robot
        .type_text_in_context(&id, "only-inner", Some(&context))
        .await
        .unwrap();
    let rows = robot
        .list_elements_query(
            &id,
            ElementQuery {
                include_frames: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let editors: Vec<_> = rows
        .iter()
        .filter(|row| row.resource_id.as_deref() == Some("editor"))
        .collect();
    assert_eq!(
        editors
            .iter()
            .filter(|row| row.value.as_deref() == Some("only-inner"))
            .count(),
        1
    );
    assert!(
        editors
            .iter()
            .filter(|row| row
                .web_context
                .as_ref()
                .is_none_or(|ctx| ctx.frame_path.len() != 2))
            .all(|row| row.value.as_deref() == Some(""))
    );
    robot
        .clear_text_in_context(&id, Some(&context))
        .await
        .unwrap();
    // Same stable selector, different document and nodes. Never activate the replacement.
    robot
        .open_url(&id, &format!("http://127.0.0.1:{port}/"), None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        robot
            .tap_in_context(&id, Point { x: 0., y: 0. }, Some(&context))
            .await
            .is_err()
    );
    robot.close_session().await.unwrap();
    server.abort();
}

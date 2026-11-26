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
                    "/" => String::from(r#"<button id="root-gestures" onpointerdown="this.started=performance.now()"
                      onpointerup="this.dataset.held=Math.max(+this.dataset.held||0,performance.now()-this.started)"
                      ondblclick="this.dataset.doubles=(+this.dataset.doubles||0)+1">Root gestures</button>
                      <input id="editor"><iframe id="outer" src="/same"></iframe><iframe id="other" src="/other"></iframe>"#),
                    "/same" => format!(r#"<iframe id="inner" src="http://localhost:{port}/cross"></iframe>"#),
                    _ => r#"<input id="editor"><button id="submit" onclick="this.textContent='Paid'">Pay</button>
                      <br><button id="gestures" onpointerdown="this.started=performance.now()"
                        onpointerup="this.dataset.held=Math.max(+this.dataset.held||0,performance.now()-this.started)"
                        ondblclick="this.dataset.doubles=(+this.dataset.doubles||0)+1">Gestures</button>
                      <div style="height:1800px"></div>"#.into(),
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
    let hit = robot.cdp(&target, "Runtime.evaluate", json!({
        "expression":"(() => {const r=document.getElementById('root-gestures').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()",
        "returnByValue":true,
    })).await.unwrap();
    let hit = &hit["result"]["value"];
    let root_point = Point {
        x: hit["x"].as_f64().unwrap(),
        y: hit["y"].as_f64().unwrap(),
    };
    for gesture in [
        robot_core::PointerGesture::LongPress { duration_ms: 700 },
        robot_core::PointerGesture::DoubleTap,
    ] {
        robot
            .pointer_gesture(&id, root_point, gesture, None)
            .await
            .unwrap();
    }
    let root_events = robot.cdp(&target, "Runtime.evaluate", json!({
        "expression":"(() => {const {held,doubles}=document.getElementById('root-gestures').dataset;return {held:+held,doubles:+doubles};})()",
        "returnByValue":true,
    })).await.unwrap();
    let root_events = &root_events["result"]["value"];
    assert!(
        root_events["held"]
            .as_f64()
            .is_some_and(|value| value >= 550.),
        "root {root_events:?}"
    );
    assert_eq!(root_events["doubles"], 1, "root {root_events:?}");
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
    let gesture_context = rows
        .iter()
        .find_map(|row| {
            if row.resource_id.as_deref() != Some("gestures") {
                return None;
            }
            row.web_context
                .as_ref()
                .filter(|ctx| ctx.frame_path.len() == 2)
        })
        .unwrap();
    for gesture in [
        robot_core::PointerGesture::LongPress { duration_ms: 700 },
        robot_core::PointerGesture::DoubleTap,
    ] {
        robot
            .pointer_gesture(&id, Point { x: 0., y: 0. }, gesture, Some(gesture_context))
            .await
            .unwrap();
    }
    let (events, _) = robot.eval_in_context_value(
        &target, gesture_context,
        "(() => {const {held,doubles}=document.getElementById('gestures').dataset;return {held:+held,doubles:+doubles};})()",
        false,
    ).await.unwrap();
    let events = &events["result"]["value"];
    assert!(
        events["held"].as_f64().is_some_and(|value| value >= 550.),
        "{events:?}"
    );
    assert_eq!(events["doubles"], 1, "{events:?}");
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
    assert!(
        robot
            .pointer_gesture(
                &id,
                Point { x: 0., y: 0. },
                robot_core::PointerGesture::DoubleTap,
                Some(gesture_context)
            )
            .await
            .is_err()
    );
    robot.close_session().await.unwrap();
    server.abort();
}

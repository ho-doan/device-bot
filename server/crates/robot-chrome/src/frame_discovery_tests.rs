//! Exercise frame admission through the production CDP discovery path.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn frame(id: &str, parent: Option<&str>) -> Value {
    json!({"frameTree":{"frame":{"id":id,"parentId":parent,"url":"https://fixture/"}}})
}

fn attached(session: &str, parent: &str, kind: &str) -> Value {
    json!({"method":"Target.attachedToTarget","sessionId":parent,
        "params":{"sessionId":session,"targetInfo":{"type":kind}}})
}

async fn discover(
    trees: BTreeMap<&str, Value>,
    attachments: BTreeMap<&str, Vec<Value>>,
) -> (frames::FrameNode, Vec<frames::AuthorizedFrame>, Vec<Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let trees: BTreeMap<String, _> = trees.into_iter().map(|(k, v)| (k.into(), v)).collect();
    let mut attachments: BTreeMap<String, _> = attachments
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut requests = Vec::new();
        while let Some(Ok(message)) = socket.next().await {
            if !message.is_text() {
                continue;
            }
            let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            let session = request["sessionId"].as_str().unwrap_or("");
            let method = request["method"].as_str().unwrap();
            let result = match method {
                "Target.createBrowserContext" => json!({"browserContextId":"case"}),
                "Target.createTarget" => json!({"targetId":"page"}),
                "Target.getTargets" => json!({"targetInfos":[{"targetId":"page","type":"page","browserContextId":"case"}]}),
                "Target.attachToTarget" => json!({"sessionId":"page-session"}),
                "Page.getFrameTree" => trees
                    .get(session)
                    .expect("unexpected frame tree read")
                    .clone(),
                "Target.setAutoAttach" => {
                    for event in attachments.remove(session).unwrap_or_default() {
                        socket
                            .send(Message::Text(event.to_string().into()))
                            .await
                            .unwrap();
                    }
                    json!({})
                }
                "Page.enable"
                | "Runtime.enable"
                | "Target.detachFromTarget"
                | "Target.disposeBrowserContext" => json!({}),
                _ => panic!("unexpected CDP request: {request}"),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let finished = method == "Target.disposeBrowserContext";
            requests.push(request);
            if finished {
                break;
            }
        }
        requests
    });
    let connection = connection::BrowserConnection::connect(&endpoint)
        .await
        .unwrap();
    let mut session = connection::BrowserSession::create(Arc::new(Mutex::new(connection)), None)
        .await
        .unwrap();
    let target = serde_json::from_value(json!({"id":"page","type":"page"})).unwrap();
    let (tree, authorized) = session.discover_frames(&target).await.unwrap();
    session.close().await.unwrap();
    (tree, authorized, server.await.unwrap())
}

#[tokio::test]
async fn discovery_admits_owned_and_omitted_roots_and_late_nested_frames() {
    let mut root = frame("root", None);
    root["frameTree"]["childFrames"] = json!([{"frame":{"id":"known","parentId":"root"}}]);
    let (tree, authorized, _) = discover(
        BTreeMap::from([
            ("page-session", root),
            ("known-session", frame("known", Some("root"))),
            ("nested-session", frame("nested", Some("known"))),
            ("omitted-session", frame("omitted", Some("root"))),
            ("late-session", frame("late", Some("omitted"))),
        ]),
        BTreeMap::from([
            (
                "page-session",
                vec![
                    attached("nested-session", "known-session", "iframe"),
                    attached("known-session", "page-session", "iframe"),
                    attached("omitted-session", "page-session", "iframe"),
                ],
            ),
            (
                "omitted-session",
                vec![attached("late-session", "omitted-session", "iframe")],
            ),
        ]),
    )
    .await;
    assert_eq!(
        authorized
            .iter()
            .map(|f| f.frame_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["known", "nested", "omitted", "late"])
    );
    assert_eq!(
        frames::frame_chain(&tree, "late")
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        vec!["omitted", "late"]
    );
}

#[tokio::test]
async fn discovery_rejects_foreign_roots_and_never_queries_untrusted_parents_or_workers() {
    let (_, authorized, requests) = discover(
        BTreeMap::from([
            ("page-session", frame("root", None)),
            ("foreign-session", frame("foreign", Some("another-page"))),
            ("malformed-session", json!({})),
        ]),
        BTreeMap::from([(
            "page-session",
            vec![
                attached("foreign-session", "page-session", "iframe"),
                attached("malformed-session", "page-session", "iframe"),
                attached("orphan-session", "untrusted-session", "iframe"),
                attached("worker-session", "page-session", "service_worker"),
                attached("", "page-session", "iframe"),
            ],
        )]),
    )
    .await;
    assert!(authorized.is_empty());
    let queried: BTreeSet<_> = requests
        .iter()
        .filter(|r| r["method"] == "Page.getFrameTree")
        .filter_map(|r| r["sessionId"].as_str())
        .collect();
    assert_eq!(
        queried,
        BTreeSet::from(["page-session", "foreign-session", "malformed-session"])
    );
    let detached: BTreeSet<_> = requests
        .iter()
        .filter(|r| r["method"] == "Target.detachFromTarget")
        .filter_map(|r| r["params"]["sessionId"].as_str())
        .collect();
    assert_eq!(
        detached,
        BTreeSet::from(["foreign-session", "malformed-session"])
    );
}

//! Optional, authenticated discovery transport shared by native drivers.
//! Device endpoints are explicit and keyed by the full device ID. Desktop
//! discovery can also read a manifest for an already identified process.
use robot_core::{Bounds, UiElement};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

pub fn directory() -> PathBuf {
    std::env::temp_dir().join("device-bot-flutter")
}

pub async fn configured(device_id: &str) -> Option<Vec<UiElement>> {
    let config: Value =
        serde_json::from_str(&std::env::var("DEVICE_BOT_FLUTTER_ENDPOINTS").ok()?).ok()?;
    let endpoint = config.get(device_id)?;
    elements(endpoint, None).await
}

pub async fn for_process(process_id: u32) -> Option<Vec<UiElement>> {
    tokio::time::timeout(Duration::from_secs(1), async {
        let file = tokio::fs::File::open(directory().join(format!("{process_id}.json")))
            .await
            .ok()?;
        let mut bytes = vec![];
        file.take(8192).read_to_end(&mut bytes).await.ok()?;
        let manifest: Value = serde_json::from_slice(&bytes).ok()?;
        elements(&manifest, Some(process_id)).await
    })
    .await
    .ok()
    .flatten()
}

pub async fn elements(endpoint: &Value, process_id: Option<u32>) -> Option<Vec<UiElement>> {
    tokio::time::timeout(Duration::from_secs(1), async {
        if endpoint.get("version")?.as_u64()? != 1 {
            return None;
        }
        if let Some(id) = process_id {
            if endpoint.get("processId")?.as_u64()? != u64::from(id) {
                return None;
            }
        }
        let port = u16::try_from(endpoint.get("port")?.as_u64()?).ok()?;
        let token = endpoint.get("token")?.as_str()?;
        if port == 0 || !(32..=128).contains(&token.len()) {
            return None;
        }
        // Remote mobile devices must be reached through an explicit local tunnel.
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .ok()?;
        socket
            .write_all(format!("{}\n", json!({"version":1,"token":token})).as_bytes())
            .await
            .ok()?;
        let mut bytes = vec![];
        BufReader::new(socket.take(4 * 1024 * 1024))
            .read_until(b'\n', &mut bytes)
            .await
            .ok()?;
        if bytes.last() != Some(&b'\n') {
            return None;
        }
        let response: Value = serde_json::from_slice(&bytes).ok()?;
        if response.get("version")?.as_u64()? != 1 || response.get("error").is_some() {
            return None;
        }
        if let Some(id) = process_id {
            if response.get("processId")?.as_u64()? != u64::from(id) {
                return None;
            }
        }
        parse_elements(response.get("elements")?)
    })
    .await
    .ok()
    .flatten()
}

/// Parse the bridge's flat node schema, preserving explicit empty values.
pub fn parse_elements(value: &Value) -> Option<Vec<UiElement>> {
    let nodes = value.as_array()?;
    if nodes.len() > 5000 {
        return None;
    }
    nodes
        .iter()
        .map(|node| {
            let string = |key| node.get(key).and_then(Value::as_str).map(str::to_owned);
            let coordinate = |key| i32::try_from(node.get(key)?.as_i64()?).ok();
            Some(UiElement {
                class_name: {
                    let role = string("className").filter(|s| !s.is_empty())?;
                    match string("widgetType") {
                        Some(widget) => format!("{role} {widget}"),
                        None => role,
                    }
                },
                resource_id: string("resourceId"),
                text: string("text"),
                content_description: string("contentDescription"),
                bounds: Bounds {
                    left: coordinate("left")?,
                    top: coordinate("top")?,
                    right: coordinate("right")?,
                    bottom: coordinate("bottom")?,
                },
                clickable: node.get("clickable")?.as_bool()?,
                enabled: node.get("enabled")?.as_bool()?,
                focused: node.get("focused").and_then(Value::as_bool),
                checked: node.get("checked").and_then(Value::as_bool),
                selected: node.get("selected").and_then(Value::as_bool),
                value: string("value"),
                element_type: string("className").and_then(|role| {
                    robot_core::ElementType::parse(match role.as_str() {
                        "checkbox" => "checkBox",
                        "text" => "staticText",
                        other => other,
                    })
                    .ok()
                }),
                web_context: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

    fn node() -> Value {
        json!({"className":"textField","resourceId":"search","text":null,"value":"",
            "left":-20,"top":30,"right":200,"bottom":80,"clickable":true,"enabled":true})
    }

    async fn serve(response: Value) -> (Value, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = json!({"version":1,"processId":1234,"port":listener.local_addr().unwrap().port(),"token":TOKEN});
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request, json!({"version":1,"token":TOKEN}));
            socket
                .get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        });
        (endpoint, task)
    }

    #[tokio::test]
    async fn authenticated_mobile_or_desktop_snapshot_preserves_keys_empty_values_and_bounds() {
        for scope in [None, Some(1234)] {
            let (endpoint, server) =
                serve(json!({"version":1,"processId":1234,"elements":[node()]})).await;
            let nodes = elements(&endpoint, scope).await.unwrap();
            assert_eq!(nodes[0].value.as_deref(), Some(""));
            assert_eq!(nodes[0].resource_id.as_deref(), Some("search"));
            assert_eq!(nodes[0].bounds.left, -20);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn stale_process_incompatible_or_malformed_snapshots_are_unavailable() {
        for response in [
            json!({"version":1,"processId":999,"elements":[node()]}),
            json!({"version":2,"processId":1234,"elements":[node()]}),
            json!({"version":1,"processId":1234,"elements":[{}]}),
            json!({"version":1,"processId":1234,"error":"unauthorized","elements":[]}),
        ] {
            let (endpoint, server) = serve(response).await;
            assert!(elements(&endpoint, Some(1234)).await.is_none());
            server.await.unwrap();
        }
        assert!(
            elements(&json!({"version":1,"port":1,"token":"short"}), None)
                .await
                .is_none()
        );
        assert!(
            elements(
                &json!({"version":1,"port":1,"token":TOKEN,"processId":999}),
                Some(1234)
            )
            .await
            .is_none()
        );
    }

    #[test]
    fn web_snapshot_parser_rejects_incomplete_nodes_and_excessive_trees() {
        assert!(parse_elements(&json!([{}])).is_none());
        assert!(parse_elements(&json!(vec![node(); 5001])).is_none());
        assert!(parse_elements(&json!([])).unwrap().is_empty());
    }
}

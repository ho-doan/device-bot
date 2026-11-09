//! Optional, authenticated discovery transport shared by native drivers.
//! Device endpoints are explicit and keyed by the full device ID. Desktop
//! discovery can also read a manifest for an already identified process.
use std::{path::PathBuf, time::Duration};
use robot_core::{Bounds, UiElement};
use serde_json::{Value, json};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, net::TcpStream};

pub fn directory() -> PathBuf { std::env::temp_dir().join("device-bot-flutter") }

pub async fn configured(device_id: &str) -> Option<Vec<UiElement>> {
    let config: Value = serde_json::from_str(&std::env::var("DEVICE_BOT_FLUTTER_ENDPOINTS").ok()?).ok()?;
    let endpoint = config.get(device_id)?;
    elements(endpoint, None).await
}

pub async fn for_process(process_id: u32) -> Option<Vec<UiElement>> {
    tokio::time::timeout(Duration::from_secs(1), async {
        let file = tokio::fs::File::open(directory().join(format!("{process_id}.json"))).await.ok()?;
        let mut bytes = vec![];
        file.take(8192).read_to_end(&mut bytes).await.ok()?;
        let manifest: Value = serde_json::from_slice(&bytes).ok()?;
        elements(&manifest, Some(process_id)).await
    }).await.ok().flatten()
}

pub async fn elements(endpoint: &Value, process_id: Option<u32>) -> Option<Vec<UiElement>> {
    tokio::time::timeout(Duration::from_secs(1), async {
        if endpoint.get("version")?.as_u64()? != 1 { return None; }
        if let Some(id) = process_id {
            if endpoint.get("processId")?.as_u64()? != u64::from(id) { return None; }
        }
        let port = u16::try_from(endpoint.get("port")?.as_u64()?).ok()?;
        let token = endpoint.get("token")?.as_str()?;
        if port == 0 || !(32..=128).contains(&token.len()) { return None; }
        // Remote mobile devices must be reached through an explicit local tunnel.
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await.ok()?;
        socket.write_all(format!("{}\n", json!({"version":1,"token":token})).as_bytes()).await.ok()?;
        let mut bytes = vec![];
        BufReader::new(socket.take(4 * 1024 * 1024)).read_until(b'\n', &mut bytes).await.ok()?;
        if bytes.last() != Some(&b'\n') { return None; }
        let response: Value = serde_json::from_slice(&bytes).ok()?;
        if response.get("version")?.as_u64()? != 1 || response.get("error").is_some() { return None; }
        if let Some(id) = process_id {
            if response.get("processId")?.as_u64()? != u64::from(id) { return None; }
        }
        parse_elements(response.get("elements")?)
    }).await.ok().flatten()
}

/// Parse the bridge's flat node schema, preserving explicit empty values.
pub fn parse_elements(value: &Value) -> Option<Vec<UiElement>> {
    let nodes = value.as_array()?;
    if nodes.len() > 5000 { return None; }
    nodes.iter().map(|node| {
        let string = |key| node.get(key).and_then(Value::as_str).map(str::to_owned);
        let coordinate = |key| i32::try_from(node.get(key)?.as_i64()?).ok();
        Some(UiElement {
            class_name: string("className").filter(|s| !s.is_empty())?,
            resource_id: string("resourceId"), text: string("text"), content_description: string("contentDescription"),
            bounds: Bounds { left: coordinate("left")?, top: coordinate("top")?, right: coordinate("right")?, bottom: coordinate("bottom")? },
            clickable: node.get("clickable")?.as_bool()?, enabled: node.get("enabled")?.as_bool()?,
            focused: node.get("focused").and_then(Value::as_bool), checked: node.get("checked").and_then(Value::as_bool), selected: node.get("selected").and_then(Value::as_bool),
            value: string("value"), element_type: None,
        })
    }).collect()
}

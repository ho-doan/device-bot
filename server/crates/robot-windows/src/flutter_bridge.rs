//! Optional, process-scoped Flutter widget-tree discovery. A missing, stale,
//! incompatible, or unresponsive bridge leaves native UIA available.

use std::time::Duration;

use robot_core::RobotError;
#[cfg(test)]
use robot_core::UiElement;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

pub(super) fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join("device-bot-flutter")
}

async fn read_manifest(process_id: u32) -> Option<Value> {
    let file = tokio::fs::File::open(directory().join(format!("{process_id}.json")))
        .await
        .ok()?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// False means no registration. Once a request is sent, failures are surfaced
/// instead of replaying a potentially completed action via another input path.
pub(super) async fn action(process_id: u32, command: Value) -> Result<bool, RobotError> {
    let Some(manifest) = read_manifest(process_id).await else {
        return Ok(false);
    };
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        request(&manifest, process_id, command),
    )
    .await
    .ok()
    .flatten()
    .ok_or_else(|| {
        RobotError::CommandFailed("Flutter widget action did not return a valid response".into())
    })?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(RobotError::CommandFailed(format!(
            "Flutter widget action: {error}"
        )));
    }
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(RobotError::CommandFailed(
            "Flutter bridge did not acknowledge the action".into(),
        ));
    }
    Ok(true)
}

#[cfg(test)]
async fn query(manifest: &Value, process_id: u32) -> Option<Vec<UiElement>> {
    let response = request(manifest, process_id, json!({})).await?;
    if !response.get("elements")?.is_array() {
        return None;
    }
    let elements =
        super::parse_uia_elements(&serde_json::to_vec(response.get("elements")?).ok()?).ok()?;
    (elements.len() <= super::MAX_UI_NODES).then_some(elements)
}

async fn request(manifest: &Value, process_id: u32, mut command: Value) -> Option<Value> {
    if manifest.get("version")?.as_u64()? != 1
        || manifest.get("processId")?.as_u64()? != u64::from(process_id)
    {
        return None;
    }
    let port = u16::try_from(manifest.get("port")?.as_u64()?).ok()?;
    let token = manifest.get("token")?.as_str()?;
    if port == 0 || token.len() < 32 || token.len() > 128 {
        return None;
    }
    // Never connect to an arbitrary host supplied by a manifest.
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .ok()?;
    command["version"] = json!(1);
    command["token"] = json!(token);
    let mut request = serde_json::to_vec(&command).ok()?;
    request.push(b'\n');
    stream.write_all(&request).await.ok()?;
    let mut response = Vec::new();
    BufReader::new(stream.take(MAX_RESPONSE_BYTES))
        .read_until(b'\n', &mut response)
        .await
        .ok()?;
    if response.last() != Some(&b'\n') {
        return None;
    }
    let response: Value = serde_json::from_slice(&response).ok()?;
    if response.get("version")?.as_u64()? != 1
        || response.get("processId")?.as_u64()? != u64::from(process_id)
    {
        return None;
    }
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn serve(response: Value) -> (Value, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let manifest = json!({
            "version": 1, "processId": 1234,
            "port": listener.local_addr().unwrap().port(),
            "token": "abcdefghijklmnopqrstuvwxyz0123456789",
        });
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut request = String::new();
            stream.read_line(&mut request).await.unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["version"], 1);
            assert_eq!(request["token"], "abcdefghijklmnopqrstuvwxyz0123456789");
            let bytes = format!("{response}\n");
            stream.get_mut().write_all(bytes.as_bytes()).await.unwrap();
        });
        (manifest, task)
    }

    #[tokio::test]
    async fn discovers_flutter_elements_with_desktop_bounds_and_state() {
        let (manifest, task) = serve(json!({
            "version": 1, "processId": 1234,
            "elements": [{
                "className": "textField", "resourceId": "email-field",
                "text": "Email", "value": "alex@example.com", "focused": true,
                "left": -120, "top": 100, "right": 220, "bottom": 150,
                "enabled": true, "clickable": true,
            }],
        }))
        .await;
        let nodes = query(&manifest, 1234).await.unwrap();
        assert_eq!(nodes[0].resource_id.as_deref(), Some("email-field"));
        assert_eq!(nodes[0].bounds.left, -120);
        assert_eq!(nodes[0].focused, Some(true));
        assert_eq!(nodes[0].value.as_deref(), Some("alex@example.com"));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn empty_flutter_tree_is_a_successful_snapshot() {
        let (manifest, task) = serve(json!({
            "version": 1, "processId": 1234, "elements": [],
        }))
        .await;
        assert!(query(&manifest, 1234).await.unwrap().is_empty());
        task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_manifest_for_another_process_before_connecting() {
        assert!(
            query(
                &json!({
                    "version": 1, "processId": 5678, "port": 1,
                    "token": "abcdefghijklmnopqrstuvwxyz0123456789",
                }),
                1234
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_stale_or_incompatible_response() {
        for response in [
            json!({"version": 1, "processId": 5678, "elements": []}),
            json!({"version": 2, "processId": 1234, "elements": []}),
            json!({"version": 1, "processId": 1234, "elements": [{}]}),
        ] {
            let (manifest, task) = serve(response).await;
            assert!(query(&manifest, 1234).await.is_none());
            task.await.unwrap();
        }
    }
}

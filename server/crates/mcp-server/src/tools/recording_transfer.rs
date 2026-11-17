//! Bounded reads of recordings finalized by this MCP session only.
use base64::{Engine, engine::general_purpose::STANDARD};
use device_mcp_protocol::{ToolDefinition, ToolResult};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    sync::Mutex,
};

pub(super) const NAME: &str = "mobile_read_screen_recording_chunk";
const CHUNK: usize = 64 * 1024;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Default)]
pub(super) struct RecordingTransfers(Mutex<HashMap<String, (tokio::fs::File, u64)>>);
impl RecordingTransfers {
    pub(super) async fn register(&self, path: &Path) -> std::io::Result<String> {
        let file = tokio::fs::File::open(path).await?;
        let meta = file.metadata().await?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "recording size is invalid or exceeds transfer limit",
            ));
        }
        let mut records = self.0.lock().await;
        if records.len() >= 16 {
            return Err(std::io::Error::other("recording transfer limit reached"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        records.insert(id.clone(), (file, meta.len()));
        Ok(id)
    }
    pub(super) async fn read(&self, args: &Value) -> ToolResult {
        let Some(object) = args.as_object() else {
            return ToolResult::execution_error("invalid recording transfer arguments");
        };
        if object.len() != 2
            || !object.contains_key("recordingId")
            || !object.contains_key("offset")
        {
            return ToolResult::execution_error("invalid recording transfer arguments");
        }
        let Some(id) = args["recordingId"].as_str().filter(|s| !s.is_empty()) else {
            return ToolResult::execution_error("invalid recording device");
        };
        let Some(offset) = args["offset"].as_u64() else {
            return ToolResult::execution_error("invalid recording offset");
        };
        let mut guard = self.0.lock().await;
        let Some((file, size)) = guard.get_mut(id) else {
            return ToolResult::execution_error("no finalized recording in this session");
        };
        if offset > *size {
            return ToolResult::execution_error("recording offset exceeds size");
        }
        if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
            return ToolResult::execution_error("recording seek failed");
        }
        let mut bytes = vec![0; ((*size - offset) as usize).min(CHUNK)];
        if file.read_exact(&mut bytes).await.is_err() {
            return ToolResult::execution_error("recording read failed");
        }
        ToolResult::structured(json!({"offset":offset,"size":*size,"data":STANDARD.encode(bytes)}))
    }
}
pub(super) fn definition() -> ToolDefinition {
    ToolDefinition { name: NAME.into(), title: Some("Read finalized session recording".into()), description: "Read a bounded chunk from a recording finalized in this MCP session; no filesystem paths are accepted.".into(), input_schema: json!({"type":"object","properties":{"recordingId":{"type":"string","minLength":1},"offset":{"type":"integer","minimum":0}},"required":["recordingId","offset"],"additionalProperties":false}) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn chunks_are_bounded_and_private_to_the_session() {
        let path =
            std::env::temp_dir().join(format!("recording-transfer-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, vec![7; CHUNK + 3]).unwrap();
        let owner = RecordingTransfers::default();
        let other = RecordingTransfers::default();
        let id = owner.register(&path).await.unwrap();
        assert_eq!(
            other
                .read(&json!({"recordingId":id,"offset":0}))
                .await
                .is_error,
            Some(true)
        );
        assert_eq!(
            owner
                .read(&json!({"recordingId":id,"offset":0,"path":path}))
                .await
                .is_error,
            Some(true)
        );
        let first = owner
            .read(&json!({"recordingId":id,"offset":0}))
            .await
            .structured_content
            .unwrap();
        assert_eq!(
            STANDARD
                .decode(first["data"].as_str().unwrap())
                .unwrap()
                .len(),
            CHUNK
        );
        let last = owner
            .read(&json!({"recordingId":id,"offset":CHUNK}))
            .await
            .structured_content
            .unwrap();
        assert_eq!(
            STANDARD.decode(last["data"].as_str().unwrap()).unwrap(),
            vec![7; 3]
        );
        assert_eq!(
            owner
                .read(&json!({"recordingId":id,"offset":CHUNK + 4}))
                .await
                .is_error,
            Some(true)
        );
        std::fs::remove_file(path).unwrap();
    }
}

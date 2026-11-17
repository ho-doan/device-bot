//! Evidence paths, recording cleanup, snapshots, and secret redaction.

use super::{FlowReport, FlowRunner};
use crate::mcp::Result;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

impl FlowRunner {
    /// Store the resolved configuration alongside evidence; sensitive inputs
    /// use the same redaction rules as MCP traces and query snapshots.
    pub fn save_execution_snapshot(&self) -> Result<()> {
        let Some(mut snapshot) = self.execution_snapshot.clone() else {
            return Ok(());
        };
        self.scrub(&mut snapshot);
        fs::create_dir_all(&self.directory).map_err(|e| crate::FlowError::from(e.to_string()))?;
        for (key, name) in [
            ("profile", "resolved-profile.yaml"),
            ("flow", "expanded-flow.yaml"),
        ] {
            let yaml = serde_yaml::to_string(&snapshot[key])
                .map_err(|e| crate::FlowError::from(e.to_string()))?;
            fs::write(self.directory.join(name), yaml)
                .map_err(|e| crate::FlowError::from(e.to_string()))?;
        }
        Ok(())
    }

    pub(super) async fn stop_recording(&mut self) -> Result<()> {
        if !self.recording {
            return Err("no active recording".into());
        }
        self.recording = false;
        if let Some(overlay) = self.video_overlay.as_mut() {
            overlay.finish();
        }
        let recording_device = self
            .recording_device
            .take()
            .ok_or("recording device missing")?;
        let result = self
            .call(
                "mobile_stop_screen_recording",
                json!({"deviceId":recording_device}),
            )
            .await?;
        let path = result["content"]
            .as_array()
            .and_then(|c| {
                c.iter()
                    .filter_map(|v| v["text"].as_str())
                    .find_map(|t| t.strip_prefix("Screen recording saved: "))
            })
            .ok_or("recorded video path missing")?;
        let path = Path::new(path.trim());
        if fs::metadata(path)
            .map_err(|e| crate::FlowError::from(e.to_string()))?
            .len()
            == 0
        {
            return Err("recorded video is empty".into());
        }
        fs::copy(path, self.directory.join("video.mp4"))
            .map_err(|e| crate::FlowError::from(e.to_string()))?;
        Ok(())
    }

    /// Render step/action/test-input captions into the finalized CLI video.
    /// Caption input stays in memory and is not serialized into the flow report.
    pub async fn annotate_video(&mut self, report: &FlowReport) -> Result<()> {
        if let Some(overlay) = &self.video_overlay {
            overlay
                .render(&self.directory, &report.steps, &report.status)
                .await?;
        }
        Ok(())
    }

    pub(super) fn artifact_path(&self, name: &str) -> Result<PathBuf> {
        let path = Path::new(name);
        if self.version == 1 {
            return Ok(path.to_path_buf());
        }
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err("artifact path must stay inside artifact directory".into());
        }
        Ok(self.directory.join(path))
    }
    pub(super) fn redact(&self, text: &str) -> String {
        let mut result = text.to_owned();
        let mut secrets = self.secrets.clone();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for secret in secrets {
            if !secret.is_empty() {
                result = result.replace(&secret, "<REDACTED>");
            }
        }
        result
    }
    pub(super) fn scrub(&self, value: &mut Value) {
        match value {
            Value::String(t) => *t = self.redact(t),
            Value::Array(items) => {
                for v in items {
                    self.scrub(v)
                }
            }
            Value::Object(items) => {
                for v in items.values_mut() {
                    self.scrub(v)
                }
            }
            _ => {}
        }
    }
}

pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| crate::FlowError::from(e.to_string()))?;
    }
    fs::write(path, bytes).map_err(|e| crate::FlowError::from(e.to_string()))
}

pub(super) fn collect_secrets(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(call) = map.get("call_tool").and_then(Value::as_object) {
                if call.get("name").and_then(Value::as_str) == Some("mobile_start_observation") {
                    if let Some(values) = call
                        .get("arguments")
                        .and_then(|v| v.get("redact"))
                        .and_then(Value::as_array)
                    {
                        out.extend(
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned),
                        );
                    }
                }
            }
            for key in ["text", "expected"] {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    if !s.is_empty() {
                        out.push(s.into());
                    }
                }
            }
            for v in map.values() {
                collect_secrets(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_secrets(v, out)
            }
        }
        _ => {}
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationOptions {
    #[serde(default)]
    pub include_text: bool,
    #[serde(default)]
    pub redact: Vec<String>,
}

impl std::fmt::Debug for ObservationOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservationOptions")
            .field("include_text", &self.include_text)
            .field("redact_count", &self.redact.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::ObservationOptions;

    #[test]
    fn debug_hides_redaction_values() {
        let options = ObservationOptions {
            include_text: true,
            redact: vec!["sensitive-value".into()],
        };
        let output = format!("{options:?}");
        assert!(!output.contains("sensitive-value"));
        assert!(output.contains("redact_count: 1"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationRecord {
    pub sequence: u64,
    pub elapsed_ms: u64,
    pub page_id: String,
    pub frame_id: Option<String>,
    pub kind: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationBatch {
    pub records: Vec<ObservationRecord>,
    pub dropped: u64,
    pub closed: bool,
    pub terminal_reason: Option<String>,
    pub limitations: Vec<String>,
}

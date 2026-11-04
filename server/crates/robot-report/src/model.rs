use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source_dir: String,
    pub created_at: u64,
}

/// The serialized robot-flow contract; no runner or platform dependency.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaseReport {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub selected_device: Option<Value>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
}

impl CaseReport {
    pub fn effective_status(&self) -> &str {
        if self.error.is_some() || self.steps.iter().any(|step| step.status == "failed") {
            "FAIL"
        } else if self.dry_run {
            "DRY_RUN"
        } else {
            &self.status
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Step {
    pub id: String,
    pub action: String,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub id: usize,
    pub name: String,
    pub kind: String,
    pub bytes: u64,
    /// Relative to the immutable run directory. Never accepted from HTTP input.
    pub stored_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub id: usize,
    pub report: CaseReport,
    pub duration_seconds: Option<f64>,
    pub files: Vec<Evidence>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub total: usize,
    pub passed: usize,
    pub partial: usize,
    pub failed: usize,
    pub not_run: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub imported_at: u64,
    pub first_report_at: u64,
    pub last_report_at: u64,
    pub counts: Counts,
    pub cases: Vec<Case>,
    pub warnings: Vec<String>,
}

impl Run {
    pub fn summary(&self) -> RunSummary {
        RunSummary {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            name: self.name.clone(),
            imported_at: self.imported_at,
            first_report_at: self.first_report_at,
            last_report_at: self.last_report_at,
            counts: self.counts.clone(),
            warnings: self.warnings.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub imported_at: u64,
    pub first_report_at: u64,
    pub last_report_at: u64,
    pub counts: Counts,
    pub warnings: Vec<String>,
}

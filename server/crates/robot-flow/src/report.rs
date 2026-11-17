//! Persist the same JSON and JUnit evidence for CLI and project cases.
use crate::FlowReport;
use std::{fs, path::Path};

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn save_report(report: &FlowReport, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("report.json"),
        serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let failure = if let Some(error) = &report.error {
        format!(
            "<{} message=\"{}\"/>",
            if report.error_kind.as_deref() == Some("assertion") {
                "failure"
            } else {
                "error"
            },
            escape(error)
        )
    } else if report.status == "PARTIAL" || report.dry_run {
        format!(
            "<skipped message=\"{}\"/>",
            escape(&if report.dry_run {
                "dry-run only; no device coverage".into()
            } else {
                report.limitations.join("; ")
            })
        )
    } else {
        String::new()
    };
    let junit = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><testsuite name=\"robot-flow\" tests=\"1\" errors=\"{}\" failures=\"{}\" skipped=\"{}\"><testcase name=\"{}\">{failure}</testcase></testsuite>",
        usize::from(report.error.is_some() && report.error_kind.as_deref() != Some("assertion")),
        usize::from(report.error_kind.as_deref() == Some("assertion")),
        usize::from(report.status == "PARTIAL" || report.dry_run),
        escape(&report.name)
    );
    fs::write(dir.join("junit.xml"), junit).map_err(|e| e.to_string())
}

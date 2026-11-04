//! Results of bounded, read-only device/session health probes.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingOwner {
    pub pid: u32,
    pub process: String,
    pub path: String,
    pub session_process_id: Option<u32>,
    pub session_process_running: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingCheck {
    pub check: HealthCheck,
    pub owners: Vec<RecordingOwner>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub scope: &'static str,
    pub device_id: String,
    pub status: &'static str,
    pub ready_for_new_session: bool,
    pub elapsed_ms: u128,
    pub checks: Vec<HealthCheck>,
    pub recording_owners: Vec<RecordingOwner>,
    pub ui_agent_probe: &'static str,
    pub recommended_actions: Vec<&'static str>,
}

impl HealthReport {
    pub fn from_checks(
        scope: &'static str,
        device_id: &str,
        elapsed_ms: u128,
        checks: Vec<HealthCheck>,
        recording_owners: Vec<RecordingOwner>,
    ) -> Self {
        let failed = checks.iter().any(|c| c.status == CheckStatus::Fail);
        let ready = !checks.is_empty() && checks.iter().all(|c| c.status == CheckStatus::Pass);
        let mut actions = Vec::new();
        for c in checks.iter().filter(|c| c.status != CheckStatus::Pass) {
            let action = match c.name.as_str() {
                "recording_session" if c.status == CheckStatus::Fail => {
                    "inspect_recording_owner; reboot_simulator_only_if_orphaned"
                }
                "recording_session" => "inspect_recording_permissions_and_owner",
                "device_booted" => "boot_target_simulator",
                "device_connection" => "check_pairing_trust_usb_or_network",
                "device_heartbeat" | "display_response" => {
                    "recheck_device_services; reboot_target_only_if_explicitly_requested"
                }
                "agent_bridge" => "start_or_restart_mcp_session_then_recheck_helper",
                "agent_authentication" => "verify_shared_agent_token",
                "mcp_health_endpoint" | "agent_health_protocol" => {
                    "restart_mcp_session_with_current_build"
                }
                "agent_heartbeat" => "restart_xctest_helper; reinstall_helper_if_restart_fails",
                "xctest_products" | "xctest_assets" => "rebuild_and_reinstall_xctest_helper",
                _ => "inspect_host_services_and_permissions",
            };
            if !actions.contains(&action) {
                actions.push(action);
            }
        }
        Self {
            scope,
            device_id: device_id.into(),
            status: if failed {
                "unhealthy"
            } else if ready {
                "healthy"
            } else {
                "degraded"
            },
            ready_for_new_session: ready,
            elapsed_ms,
            checks,
            recording_owners,
            ui_agent_probe: "Live helper heartbeat is checked through the running MCP bridge; app-specific UI/event-loop health is not inferred",
            recommended_actions: actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_or_failed_checks_never_report_ready_and_recovery_targets_helper() {
        for status in [CheckStatus::Warn, CheckStatus::Unknown, CheckStatus::Fail] {
            let report = HealthReport::from_checks(
                "test",
                "device",
                0,
                vec![HealthCheck {
                    name: "agent_heartbeat".into(),
                    status,
                    detail: "not confirmed".into(),
                }],
                vec![],
            );
            assert!(!report.ready_for_new_session);
            assert_ne!(report.status, "healthy");
            assert!(report.recommended_actions[0].contains("reinstall_helper"));
            assert!(!report.recommended_actions[0].contains("reinstall_mcp"));
        }
    }
}

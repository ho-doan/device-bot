use super::*;
use robot_core::CapabilityStatus;
#[test]
fn permission_and_read_only_target_probes_gate_readiness() {
    assert_eq!(
        mac_ax_readiness(false, true).status,
        CapabilityStatus::Blocked
    );
    assert_eq!(
        mac_ax_readiness(true, false).status,
        CapabilityStatus::Blocked
    );
    assert_eq!(
        mac_ax_readiness(true, true).status,
        CapabilityStatus::Available
    );
    assert_eq!(
        mac_display_readiness(false, true).status,
        CapabilityStatus::Blocked
    );
    assert_eq!(
        mac_display_readiness(true, false).status,
        CapabilityStatus::Blocked
    );
    assert_eq!(
        mac_display_readiness(true, true).status,
        CapabilityStatus::Available
    );
}

//! Read-only, conservative capability readiness exposed by drivers.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Available,
    Blocked,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub status: CapabilityStatus,
    pub reason: String,
}
impl Capability {
    pub fn new(status: CapabilityStatus, reason: impl Into<String>) -> Self {
        Self {
            status,
            reason: reason.into(),
        }
    }
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::new(CapabilityStatus::Unknown, reason)
    }
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::new(CapabilityStatus::Blocked, reason)
    }
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::new(CapabilityStatus::Unsupported, reason)
    }
    pub fn available(reason: impl Into<String>) -> Self {
        Self::new(CapabilityStatus::Available, reason)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub discovery: Capability,
    pub launch: Capability,
    pub input: Capability,
    pub screenshot: Capability,
    pub elements: Capability,
    pub screen_recording: Capability,
    pub observation: Capability,
}
impl Default for Capabilities {
    fn default() -> Self {
        let unknown = Capability::unknown(
            "Driver has not verified this capability; configure a driver with readiness support",
        );
        Self {
            discovery: unknown.clone(),
            launch: unknown.clone(),
            input: unknown.clone(),
            screenshot: unknown.clone(),
            elements: unknown.clone(),
            screen_recording: unknown.clone(),
            observation: unknown,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    pub host_os: String,
    pub device_id: String,
    pub capabilities: Capabilities,
}
impl CapabilityReport {
    pub fn new(device_id: &str) -> Self {
        Self {
            host_os: std::env::consts::OS.into(),
            device_id: device_id.into(),
            capabilities: Capabilities::default(),
        }
    }
}

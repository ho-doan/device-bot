//! Android `adb` adapter.
//!
//! The Phase 0 contract is intentionally present before the process wrapper is
//! added. Phase 1 will implement these operations using `tokio::process` and
//! deterministic parser tests for `adb devices -l` output.

use async_trait::async_trait;
use mobile_robot_core::{Point, Robot, RobotDevice, RobotError};

#[derive(Debug, Clone)]
pub struct AndroidRobot {
    adb_path: String,
}

impl AndroidRobot {
    pub fn new(adb_path: impl Into<String>) -> Self {
        Self {
            adb_path: adb_path.into(),
        }
    }

    pub fn adb_path(&self) -> &str {
        &self.adb_path
    }
}

#[async_trait]
impl Robot for AndroidRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Err(RobotError::Unsupported(
            "Android adb driver is scheduled for Phase 1".into(),
        ))
    }

    async fn take_screenshot(&self, _device_id: &str) -> Result<Vec<u8>, RobotError> {
        Err(RobotError::Unsupported(
            "Android adb driver is scheduled for Phase 1".into(),
        ))
    }

    async fn tap(&self, _device_id: &str, _point: Point) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "Android adb driver is scheduled for Phase 1".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_configured_adb_path() {
        assert_eq!(
            AndroidRobot::new("/opt/android/adb").adb_path(),
            "/opt/android/adb"
        );
    }
}

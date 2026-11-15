//! Errors shared by device contracts and platform drivers.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RobotError {
    #[error(
        "device_locked: iPhone {0} is locked. Unlock the iPhone on the device, then retry the operation."
    )]
    DeviceLocked(String),
    #[error("device driver command failed: {0}")]
    CommandFailed(String),
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

//! ADB subprocess execution and its testable command contract.

use std::process::Stdio;

use async_trait::async_trait;
use robot_core::RobotError;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs external commands for the Android driver.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError>;

    async fn spawn_background(
        &self,
        _executable: &str,
        _args: &[String],
        _log_path: &std::path::Path,
    ) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("background process launch".into()))
    }
}

/// Real process runner used outside tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioCommandRunner;

// Discovery also runs while QC is displaying other platforms. A short-lived ADB
// or scrcpy helper must not create a console over the viewer on Windows.
pub(super) fn helper_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn spawn_background(
        &self,
        executable: &str,
        args: &[String],
        log_path: &std::path::Path,
    ) -> Result<(), RobotError> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not open emulator log: {error}"))
            })?;
        let stderr = log.try_clone().map_err(|error| {
            RobotError::CommandFailed(format!("could not clone emulator log: {error}"))
        })?;
        helper_command(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(stderr)
            .kill_on_drop(false)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start {executable}: {error}"))
            })?;
        Ok(())
    }

    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        let output = helper_command(executable)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start {executable}: {error}"))
            })?;

        Ok(CommandOutput {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

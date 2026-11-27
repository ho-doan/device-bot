//! Builds and starts the signed XCTest runner.

use std::{fs::File, net::IpAddr, path::PathBuf, process::Stdio, time::Duration};

use crate::{runner_cache::RunnerCache, signing_issue::SigningIssue};
use robot_core::RobotError;
use tokio::process::{Child, Command};

use super::validate_device_id;

pub(super) enum RunnerStartError {
    Signing(SigningIssue),
    Other(RobotError),
}

impl From<RobotError> for RunnerStartError {
    fn from(error: RobotError) -> Self {
        Self::Other(error)
    }
}

/// Launches the signed real-device XCTest command loop from the core driver.
/// Keeping this in the driver makes a fresh MCP process reproducible: it no
/// longer depends on an unrelated terminal that happened to start the agent.
#[derive(Clone, Debug)]
pub struct XCTestAgentLauncher {
    pub(super) xcodebuild_path: String,
    pub(super) project: PathBuf,
    pub(super) derived_data: PathBuf,
    pub(super) hosts: String,
    pub(super) port: u16,
    pub(super) token: Option<String>,
}

impl XCTestAgentLauncher {
    pub fn new(
        xcodebuild_path: impl Into<String>,
        project: impl Into<PathBuf>,
        derived_data: impl Into<PathBuf>,
        hosts: impl Into<String>,
        port: u16,
        token: Option<String>,
    ) -> Result<Self, RobotError> {
        let raw_hosts = hosts.into();
        let hosts = raw_hosts
            .split(',')
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .collect::<Vec<_>>();
        if hosts.is_empty()
            || hosts
                .iter()
                .any(|host| !callback_host_is_advertisable(host))
        {
            return Err(RobotError::InvalidInput(
                "DEVICE_BOT_IOS_AGENT_HOSTS must be a comma-separated list of LAN IPs or hostnames the iPhone can open; loopback, wildcard, and link-local addresses are not reachable from the device"
                    .into(),
            ));
        }
        Ok(Self {
            xcodebuild_path: xcodebuild_path.into(),
            project: project.into(),
            derived_data: derived_data.into(),
            hosts: hosts.join(","),
            port,
            token: token.filter(|token| !token.is_empty()),
        })
    }

    pub(super) fn cache(&self) -> RunnerCache<'_> {
        RunnerCache {
            project: &self.project,
            derived_data: &self.derived_data,
            simulator: false,
        }
    }

    async fn ensure_build(&self, signing_team: Option<&str>) -> Result<(), RunnerStartError> {
        let cache = self.cache();
        let version = cache.version()?;
        if cache.is_current(&version) {
            return Ok(());
        }
        let output = tokio::time::timeout(
            Duration::from_secs(180),
            Command::new(&self.xcodebuild_path)
                .args(["build-for-testing", "-allowProvisioningUpdates", "-project"])
                .arg(&self.project)
                .args([
                    "-scheme",
                    "MobileMcpRealRunner",
                    "-destination",
                    "generic/platform=iOS",
                    "-derivedDataPath",
                ])
                .arg(&self.derived_data)
                .arg(format!("DEVICE_BOT_BUILD_ID={version}"))
                .env_remove("SDKROOT")
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| RobotError::CommandFailed("iOS XCTest build timed out after 180s".into()))?
        .map_err(|e| {
            RobotError::CommandFailed(format!("could not build iOS XCTest helper: {e}"))
        })?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if let Some(issue) =
                SigningIssue::from_xcode(&format!("{stdout}\n{stderr}"), signing_team)
            {
                return Err(RunnerStartError::Signing(issue));
            }
            let errors = stdout
                .lines()
                .filter(|line| line.contains("error:"))
                .take(8)
                .collect::<Vec<_>>()
                .join("\n");
            return Err(RobotError::CommandFailed(format!(
                "iOS XCTest build failed: {errors}\n{stderr}"
            ))
            .into());
        }
        Ok(cache.mark_current(&version)?)
    }

    pub(super) async fn start(
        &self,
        device_id: &str,
        signing_team: Option<&str>,
    ) -> Result<Child, RunnerStartError> {
        validate_device_id(device_id)?;
        self.ensure_build(signing_team).await?;
        let project = self.project.to_string_lossy().into_owned();
        let derived_data = self.derived_data.to_string_lossy().into_owned();
        let destination = format!("platform=iOS,id={device_id}");
        let mut command = Command::new(&self.xcodebuild_path);
        command
            .args([
                "test-without-building",
                "-project",
                &project,
                "-scheme",
                "MobileMcpRealRunner",
                "-destination",
                &destination,
                "-derivedDataPath",
                &derived_data,
                "-only-testing:MobileMcpRealRunnerUITests/MobileMcpRealRunnerUITests/testServe",
            ])
            .env_remove("SDKROOT")
            .stdin(Stdio::null())
            .env("TEST_RUNNER_DEVICE_BOT_DEVICE_ID", device_id);
        #[cfg(unix)]
        command.process_group(0);
        // Xcode strips TEST_RUNNER_ and injects the remainder into the
        // on-device XCTest process. The runner reads DEVICE_BOT_*.
        for (key, value) in self.test_environment() {
            command.env(key, value);
        }
        let log_path = self
            .derived_data
            .join("Logs/Agent")
            .join(format!("{device_id}-xcodebuild.log"));
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create XCTest agent log directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let stdout = File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not open XCTest agent log {}: {error}",
                    log_path.display()
                ))
            })?;
        let stderr = stdout.try_clone().map_err(|error| {
            RobotError::CommandFailed(format!("could not duplicate XCTest agent log: {error}"))
        })?;
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        Ok(command.spawn().map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not start the real-iOS XCTest agent: {error}"
            ))
        })?)
    }

    pub(super) fn test_environment(&self) -> Vec<(String, String)> {
        let mut environment = vec![
            ("TEST_RUNNER_DEVICE_BOT_HOSTS".into(), self.hosts.clone()),
            ("TEST_RUNNER_DEVICE_BOT_PORT".into(), self.port.to_string()),
        ];
        if let Some(token) = &self.token {
            environment.push(("TEST_RUNNER_DEVICE_BOT_TOKEN".into(), token.clone()));
        }
        environment
    }
}

fn callback_host_is_advertisable(host: &str) -> bool {
    if host.is_empty()
        || host.contains("//")
        || host.contains('/')
        || host.chars().any(char::is_whitespace)
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_link_local()
                && !address.is_multicast()
        }
        Ok(IpAddr::V6(address)) => {
            !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_unicast_link_local()
                && !address.is_multicast()
        }
        Err(_) => true,
    }
}

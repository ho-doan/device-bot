//! Explicit per-case fixture preparation for path-only projects.
use crate::{Project, invalid, plan::ProjectPlan};
use serde::Deserialize;
use std::{io, path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

fn default_timeout() -> u64 {
    180
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hook {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}

impl Hook {
    pub(crate) fn validate(&self, plan: &ProjectPlan) -> io::Result<()> {
        if !plan.is_legacy() {
            return Err(invalid(
                "before_each currently requires a path-only project",
            ));
        }
        if self.command.trim().is_empty()
            || self.timeout_seconds == 0
            || self.timeout_seconds > 3600
        {
            return Err(invalid(
                "before_each requires a command and timeout_seconds between 1 and 3600",
            ));
        }
        Ok(())
    }

    pub(crate) async fn run(
        &self,
        project: &Project,
        platform: Option<&str>,
        artifact: Option<&str>,
        flow: &Path,
        directory: &Path,
    ) -> io::Result<()> {
        let mut command = Command::new(project.resolve(&self.command)?);
        for argument in &self.args {
            command.arg(project.resolve(argument)?);
        }
        let case_name = flow
            .file_stem()
            .ok_or_else(|| invalid("flow has no case name"))?;
        command
            .current_dir(&project.root)
            .envs(&project.environment)
            .env("DEVICE_BOT_CASE_NAME", case_name)
            .env("DEVICE_BOT_CASE_PATH", flow)
            .env("DEVICE_BOT_CASE_DIR", directory)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(platform) = platform {
            command.env("DEVICE_BOT_PLATFORM", platform);
        } else {
            command.env_remove("DEVICE_BOT_PLATFORM");
        }
        if let Some(artifact) = artifact {
            command.env("DEVICE_BOT_ARTIFACT", artifact);
        } else {
            command.env_remove("DEVICE_BOT_ARTIFACT");
        }
        let mut child = command
            .spawn()
            .map_err(|_| io::Error::other("could not start before_each fixture preparation"))?;
        let status = tokio::select! {
            result = child.wait() => result?,
            _ = tokio::time::sleep(Duration::from_secs(self.timeout_seconds)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(io::Error::new(io::ErrorKind::TimedOut, "before_each fixture preparation timed out"));
            }
            _ = crate::run::interrupted() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(io::Error::new(io::ErrorKind::Interrupted, "before_each fixture preparation interrupted"));
            }
        };
        if !status.success() {
            return Err(io::Error::other(format!(
                "before_each fixture preparation failed ({status})"
            )));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_stops_fixture_and_returns_a_timeout_error() {
        let root = std::env::temp_dir().join(format!("project-hook-{}", uuid::Uuid::new_v4()));
        crate::init(&root).unwrap();
        let project = Project::load(&root).unwrap();
        let hook: Hook = serde_yaml::from_str(
            "command: /bin/sh\nargs: [-c, 'exec sleep 30']\ntimeout_seconds: 1\n",
        )
        .unwrap();
        hook.validate(&project.plan).unwrap();
        let error = hook
            .run(&project, None, None, &project.flows[0], &root)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_timeouts_are_rejected() {
        let root = std::env::temp_dir().join(format!("project-hook-{}", uuid::Uuid::new_v4()));
        crate::init(&root).unwrap();
        let project = Project::load(&root).unwrap();
        for value in [0, 3601] {
            let hook: Hook =
                serde_yaml::from_str(&format!("command: echo\ntimeout_seconds: {value}\n"))
                    .unwrap();
            assert!(hook.validate(&project.plan).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

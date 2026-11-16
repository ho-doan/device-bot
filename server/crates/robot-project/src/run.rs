use crate::{Project, ensure_report, windows_fixture};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;

struct Fixture {
    app: PathBuf,
    state: PathBuf,
}
impl Fixture {
    fn stop(&self) -> io::Result<()> {
        windows_fixture::stop(&self.app, &self.state)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

async fn interrupted() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// Run each declared flow through the existing native runner, stopping on failure.
pub async fn run(
    project: &Project,
    arguments: &[String],
    runner: &Path,
    server: &Path,
    reports: &Path,
) -> io::Result<i32> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = arguments
        .windows(2)
        .filter(|pair| pair[0] == "--artifacts")
        .map(|pair| PathBuf::from(&pair[1]))
        .next_back()
        .unwrap_or_else(|| PathBuf::from("artifacts/project"));
    let output = project
        .root
        .join(base)
        .join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&output)?;
    eprintln!("Project: {}\nEvidence: {}", project.name, output.display());
    let result = run_flows(project, arguments, runner, server, reports, &output).await;
    if result.is_err() {
        ensure_report(&output.join("project-error"))?;
    }
    result
}

async fn run_flows(
    project: &Project,
    arguments: &[String],
    runner: &Path,
    server: &Path,
    reports: &Path,
    output: &Path,
) -> io::Result<i32> {
    let dry = arguments.iter().any(|arg| arg == "--dry-run");
    for (index, flow) in project.flows.iter().enumerate() {
        let directory = output.join(format!("case-{:03}", index + 1));
        fs::create_dir_all(&directory)?;
        let fixture = if cfg!(windows) && !dry {
            if let Some(app) = &project.windows_fixture {
                let fixture = Fixture {
                    app: project.root.join(project.resolve(app)?),
                    state: directory.join("windows-fixture.json"),
                };
                windows_fixture::start(&fixture.app, &fixture.state)?;
                Some(fixture)
            } else {
                None
            }
        } else {
            None
        };
        let mut command = Command::new(runner);
        command
            .current_dir(&project.root)
            .args(arguments)
            .arg("--artifacts")
            .arg(&directory)
            .arg(flow)
            .envs(&project.environment)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if project.environment_value("DEVICE_BOT_BIN").is_none() {
            command.env("DEVICE_BOT_BIN", server);
        }
        if project
            .environment_value("DEVICE_BOT_REPORT_DATA_DIR")
            .is_none()
        {
            command.env("DEVICE_BOT_REPORT_DATA_DIR", reports);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                ensure_report(&directory)?;
                return Err(error);
            }
        };
        let status = tokio::select! {
            status = child.wait() => status?,
            _ = interrupted() => {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    // SAFETY: signal only this child process, never a name/group.
                    unsafe { libc::kill(pid as i32, libc::SIGTERM); }
                }
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
                ensure_report(&directory.join("interrupted"))?;
                return Err(io::Error::new(io::ErrorKind::Interrupted, "project interrupted"));
            }
        };
        if let Some(fixture) = fixture {
            fixture.stop()?;
        }
        let missing = ensure_report(&directory)?;
        if !status.success() {
            return Ok(status.code().unwrap_or(1));
        }
        if missing {
            return Err(io::Error::other("flow exited without a JUnit report"));
        }
    }
    Ok(0)
}

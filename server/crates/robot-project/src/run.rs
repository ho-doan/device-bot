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

pub(crate) async fn interrupted() {
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
    let mut selection = match crate::platform::arguments(project, arguments) {
        Ok(selection) => selection,
        Err(error) => {
            let output = create_output(project, arguments)?;
            ensure_report(&output.join("project-error"))?;
            return Err(error);
        }
    };
    let arguments = &selection.arguments;
    if !project.plan.is_legacy() {
        return run_named(project, arguments, server, reports).await;
    }
    let output = create_output(project, arguments)?;
    let dry = selection
        .arguments
        .iter()
        .any(|argument| argument == "--dry-run");
    let (_artifact, _web_server) = if !dry {
        if let Some(config) = &selection.file_preparation {
            let source = selection
                .artifact
                .as_deref()
                .expect("artifact validated during selection");
            let result = tokio::select! {
                result = config.acquire(project, source, &output) => result,
                _ = interrupted() => Err(io::Error::new(io::ErrorKind::Interrupted, "artifact preparation interrupted")),
            };
            let acquired = match result {
                Ok(acquired) => acquired,
                Err(error) => {
                    ensure_report(&output.join("project-error"))?;
                    return Err(error);
                }
            };
            let path = acquired
                .path
                .to_str()
                .ok_or_else(|| invalid_artifact_path(&output))?
                .to_owned();
            selection
                .environment
                .insert(config.path_env.clone(), path.clone());
            for index in &selection.file_argument_indexes {
                let argument = &mut selection.arguments[*index];
                *argument = argument.replace(crate::artifact::DRY_RUN_PLACEHOLDER, &path);
            }
            let web_server = if let Some(web) = &config.serve_web {
                let server = match crate::web_artifact::WebServer::start(&acquired.path).await {
                    Ok(server) => server,
                    Err(error) => {
                        ensure_report(&output.join("project-error"))?;
                        return Err(error);
                    }
                };
                selection
                    .environment
                    .insert(web.base_url_env.clone(), server.url.clone());
                for index in &selection.web_argument_indexes {
                    let argument = &mut selection.arguments[*index];
                    *argument = argument.replace(crate::web_artifact::DRY_RUN_URL, &server.url);
                }
                Some(server)
            } else {
                None
            };
            (Some(acquired), web_server)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };
    let result = run_flows(project, &selection, runner, server, reports, &output).await;
    if result.is_err() {
        ensure_report(&output.join("project-error"))?;
    }
    result
}

fn create_output(project: &Project, arguments: &[String]) -> io::Result<PathBuf> {
    let mut base = PathBuf::from("artifacts/project");
    let mut i = 0;
    while i < arguments.len() {
        let takes_value = matches!(
            arguments[i].as_str(),
            "--artifacts"
                | "--platform"
                | "--artifact"
                | "--profiles"
                | "--target"
                | "--device"
                | "--app-id"
                | "--server"
                | "--server-arg"
                | "--mcp-url"
                | "--confirm"
                | "--rpc-timeout"
                | "--case-timeout"
        );
        if takes_value {
            if let Some(value) = arguments.get(i + 1) {
                if arguments[i] == "--artifacts" && !value.is_empty() && !value.starts_with('-') {
                    base = value.into();
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output = project
        .root
        .join(base)
        .join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&output)?;
    eprintln!("Project: {}\nEvidence: {}", project.name, output.display());
    Ok(output)
}

fn invalid_artifact_path(output: &Path) -> io::Error {
    let _ = ensure_report(&output.join("project-error"));
    io::Error::new(
        io::ErrorKind::InvalidData,
        "artifact temporary path is not UTF-8",
    )
}

async fn run_flows(
    project: &Project,
    selection: &crate::platform::Selection,
    runner: &Path,
    server: &Path,
    reports: &Path,
    output: &Path,
) -> io::Result<i32> {
    let arguments = &selection.arguments;
    let dry = arguments.iter().any(|arg| arg == "--dry-run");
    if let Some(setup) = &selection.before_all_flow {
        let directory = output.join("project-setup");
        fs::create_dir_all(&directory)?;
        let code = run_flow_process(
            project, selection, runner, server, reports, setup, &directory,
        )
        .await?;
        if code != 0 || (!dry && !setup_passed(&directory)?) {
            ensure_report(&directory)?;
            return Err(io::Error::other(
                "before_all_flow fixture preparation failed or did not pass",
            ));
        }
    }
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
        if let Some(setup) = &selection.before_each_flow {
            let setup_dir = directory.join("fixture-setup");
            fs::create_dir_all(&setup_dir)?;
            let code = run_flow_process(
                project, selection, runner, server, reports, setup, &setup_dir,
            )
            .await?;
            if code != 0 || (!dry && !setup_passed(&setup_dir)?) {
                ensure_report(&directory)?;
                return Err(io::Error::other(
                    "before_each_flow fixture preparation failed or did not pass",
                ));
            }
        }
        if !dry {
            if let Some(hook) = &project.before_each {
                if let Err(error) = hook
                    .run(
                        project,
                        selection.platform.as_deref(),
                        selection.artifact.as_deref(),
                        &selection.environment,
                        flow,
                        &directory,
                    )
                    .await
                {
                    ensure_report(&directory)?;
                    return Err(error);
                }
            }
        }
        let status = run_flow_process(
            project, selection, runner, server, reports, flow, &directory,
        )
        .await?;
        if let Some(fixture) = fixture {
            fixture.stop()?;
        }
        if status != 0 {
            return Ok(status);
        }
    }
    Ok(0)
}

fn setup_passed(directory: &Path) -> io::Result<bool> {
    let mut reports = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("case-") {
            let report = entry.path().join("report.json");
            if report.is_file() {
                reports.push(report);
            }
        }
    }
    if reports.len() != 1 {
        return Ok(false);
    }
    let Ok(bytes) = fs::read(&reports[0]) else {
        return Ok(false);
    };
    let Ok(report) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(false);
    };
    Ok(report["status"] == "PASS"
        && report["dry_run"] == false
        && report["error"].is_null()
        && report["limitations"]
            .as_array()
            .is_some_and(|limitations| limitations.is_empty()))
}

async fn run_flow_process(
    project: &Project,
    selection: &crate::platform::Selection,
    runner: &Path,
    server: &Path,
    reports: &Path,
    flow: &Path,
    directory: &Path,
) -> io::Result<i32> {
    let arguments = &selection.arguments;
    let mut command = Command::new(runner);
    command
        .current_dir(&project.root)
        .args(arguments)
        .arg("--artifacts")
        .arg(directory)
        .arg(flow)
        .envs(&project.environment)
        .envs(&selection.environment)
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
            ensure_report(directory)?;
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
    let missing = ensure_report(directory)?;
    if !status.success() {
        return Ok(status.code().unwrap_or(1));
    }
    if missing {
        return Err(io::Error::other("flow exited without a JUnit report"));
    }
    Ok(0)
}

async fn run_named(
    project: &Project,
    arguments: &[String],
    server: &Path,
    reports: &Path,
) -> io::Result<i32> {
    let mut artifacts = PathBuf::from("artifacts/project");
    let mut confirmed = Vec::new();
    let mut case_timeout = 600u64;
    let mut rpc_timeout = 180u64;
    let mut dry = false;
    let mut args = arguments.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry = true,
            "--artifacts" => {
                artifacts = args
                    .next()
                    .ok_or_else(|| crate::invalid("--artifacts requires a path"))?
                    .into()
            }
            "--confirm" => confirmed.push(
                args.next()
                    .ok_or_else(|| crate::invalid("--confirm requires a value"))?
                    .clone(),
            ),
            "--case-timeout" => {
                case_timeout = args
                    .next()
                    .ok_or_else(|| crate::invalid("--case-timeout requires seconds"))?
                    .parse()
                    .map_err(|_| crate::invalid("invalid case timeout"))?
            }
            "--rpc-timeout" => {
                rpc_timeout = args
                    .next()
                    .ok_or_else(|| crate::invalid("--rpc-timeout requires seconds"))?
                    .parse()
                    .map_err(|_| crate::invalid("invalid RPC timeout"))?
            }
            _ => {
                return Err(crate::invalid(format!(
                    "option {arg} is not supported for named projects"
                )));
            }
        }
    }
    if case_timeout == 0 || case_timeout > 3600 || rpc_timeout == 0 || rpc_timeout > 600 {
        return Err(crate::invalid("invalid project timeout"));
    }
    if dry {
        let count = crate::session::validate(project, &confirmed)
            .map_err(|e| crate::invalid(e.to_string()))?;
        eprintln!("Validated {count} project cases (dry run; no device coverage)");
        return Ok(0);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output = project
        .root
        .join(artifacts)
        .join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&output)?;
    eprintln!("Project: {}\nEvidence: {}", project.name, output.display());
    let factory = crate::session::DeviceBotFactory {
        server: server.to_owned(),
        rpc_timeout: Duration::from_secs(rpc_timeout),
        artifacts_root: output.join("resources"),
    };
    let history_root = project
        .environment_value("DEVICE_BOT_REPORT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| reports.to_owned());
    let result = crate::session::run_with_history(
        project,
        &factory,
        &output,
        &confirmed,
        Duration::from_secs(case_timeout),
        Some(&history_root),
    )
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            ensure_report(&output.join("project-error"))?;
            return Err(io::Error::other(error.to_string()));
        }
    };
    Ok(i32::from(
        result.reports.iter().any(|report| report.error.is_some()),
    ))
}

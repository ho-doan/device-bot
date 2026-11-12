//! A Device Bot Chrome profile with loopback debugging enabled at launch.
use super::*;
use std::path::PathBuf;
use tokio::process::Child;

pub(super) struct ManagedChrome {
    profile: PathBuf,
    executable: Option<PathBuf>,
    child: Option<Child>,
}

impl ManagedChrome {
    pub(super) fn new(profile: PathBuf, executable: Option<PathBuf>) -> Self {
        Self {
            profile,
            executable,
            child: None,
        }
    }

    pub(super) async fn existing_endpoint(&self) -> Option<String> {
        super::startup::active_endpoint(&self.profile.join("DevToolsActivePort")).await
    }

    pub(super) async fn start(&mut self) -> Result<String, RobotError> {
        if let Some(endpoint) = self.existing_endpoint().await {
            return Ok(endpoint);
        }
        let failure = |error| {
            RobotError::CommandFailed(format!(
                "could not start Chrome with remote debugging: {error}"
            ))
        };
        tokio::fs::create_dir_all(&self.profile)
            .await
            .map_err(|e| failure(e.to_string()))?;
        let profile = tokio::fs::canonicalize(&self.profile)
            .await
            .map_err(|e| failure(e.to_string()))?;
        let log_path = profile.join("device-bot-chrome.log");
        let running = match self.child.as_mut() {
            Some(child) => child
                .try_wait()
                .map_err(|e| failure(e.to_string()))?
                .is_none(),
            None => false,
        };
        if !running {
            self.child.take();
            let port_file = profile.join("DevToolsActivePort");
            match tokio::fs::remove_file(&port_file).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(failure(e.to_string())),
            }
            let executable = self
                .executable
                .clone()
                .map(Ok)
                .unwrap_or_else(chrome_executable)?;
            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .map_err(|e| failure(e.to_string()))?;
            let stderr = log.try_clone().map_err(|e| failure(e.to_string()))?;
            self.child = Some(
                Command::new(&executable)
                    .arg(format!("--user-data-dir={}", profile.display()))
                    .args([
                        "--remote-debugging-address=127.0.0.1",
                        "--remote-debugging-port=0",
                        "--no-first-run",
                        "--no-default-browser-check",
                        "--no-startup-window",
                    ])
                    .stdin(Stdio::null())
                    .stdout(log)
                    .stderr(stderr)
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| failure(format!("{}: {e}", executable.display())))?,
            );
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(endpoint) = self.existing_endpoint().await {
                return Ok(endpoint);
            }
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait().map_err(|e| failure(e.to_string()))? {
                    self.child.take();
                    return Err(failure(format!(
                        "Chrome exited with {status}; see {}",
                        log_path.display()
                    )));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                if let Some(mut child) = self.child.take() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
                return Err(failure(format!(
                    "debugging did not become ready within 15 seconds; see {}",
                    log_path.display()
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn chrome_executable() -> Result<PathBuf, RobotError> {
    let mut candidates = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                PathBuf::from(home)
                    .join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            );
        }
        candidates.push(PathBuf::from(
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ));
    } else if cfg!(windows) {
        for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(directory) = std::env::var_os(variable) {
                candidates
                    .push(PathBuf::from(directory).join("Google/Chrome/Application/chrome.exe"));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for executable in [
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "chrome.exe",
            ] {
                candidates.push(directory.join(executable));
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| RobotError::CommandFailed(
        "Chrome is not installed or could not be found; install Chrome or set DEVICE_BOT_CHROME_PATH to its executable".into()))
}

use super::*;

pub(super) struct ManagedBrowser {
    child: Option<std::process::Child>,
    pid: u32,
    profile: PathBuf,
}

impl ManagedBrowser {
    fn running(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            return child.try_wait().ok().flatten().is_none();
        }
        #[cfg(unix)]
        {
            return unsafe { libc::kill(self.pid as i32, 0) } == 0;
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn app_bundle(executable: &std::path::Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|e| e == "app"))
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
async fn launch_native(bundle: &std::path::Path, arguments: &[String]) -> Result<u32, RobotError> {
    // LaunchServices creates an independent, foreground GUI application rather
    // than inheriting the lifecycle of the background MCP daemon.
    const LAUNCH: &str = r#"ObjC.import('AppKit');
function run(argv) {
  var configuration = $.NSMutableDictionary.dictionary;
  configuration.setObjectForKey($(argv.slice(1)), $.NSWorkspaceLaunchConfigurationArguments);
  var app = $.NSWorkspace.sharedWorkspace.launchApplicationAtURLOptionsConfigurationError(
    $.NSURL.fileURLWithPath($(argv[0])), 0x00080000, configuration, null);
  if (!app || app.isNil()) throw new Error('LaunchServices could not open Chrome');
  return String(app.processIdentifier);
}"#;
    let output = tokio::time::timeout(
        Duration::from_secs(8),
        Command::new("/usr/bin/osascript")
            .args(["-l", "JavaScript", "-e", LAUNCH, "--"])
            .arg(bundle)
            .args(arguments)
            .output(),
    )
    .await
    .map_err(|_| RobotError::CommandFailed("Chrome native launch timed out".into()))?
    .map_err(|e| {
        RobotError::CommandFailed(format!(
            "could not launch Chrome through LaunchServices: {e}"
        ))
    })?;
    if !output.status.success() {
        return Err(RobotError::CommandFailed(format!(
            "Chrome native launch failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            RobotError::CommandFailed("Chrome native launch did not return a PID".into())
        })
}

impl Drop for ManagedBrowser {
    fn drop(&mut self) {
        // Only the browser spawned by this MCP session is owned here.
        #[cfg(unix)]
        unsafe {
            libc::kill(self.pid as i32, libc::SIGTERM);
        }
        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &self.pid.to_string(), "/T", "/F"])
            .output();
        let deadline = std::time::Instant::now() + Duration::from_millis(600);
        while self.running() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(unix)]
        if self.running() {
            unsafe {
                libc::kill(self.pid as i32, libc::SIGKILL);
            }
        }
        // Chromium children may finish their last profile writes after the parent.
        std::thread::sleep(Duration::from_millis(150));
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

fn fresh_profile() -> Result<PathBuf, RobotError> {
    let profile = std::env::temp_dir().join(format!("device-bot-chrome-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&profile).map_err(|error| {
        RobotError::CommandFailed(format!("could not create fresh Chrome profile: {error}"))
    })?;
    Ok(profile)
}

impl ChromeRobot {
    pub(super) fn debug_address(&self) -> String {
        self.managed_address
            .read()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.debug_address.clone())
    }

    pub(super) fn offline_browser(&self) -> RobotDevice {
        RobotDevice {
            id: "chrome:browser".into(),
            name: if self.fresh_browser {
                "Chrome — start fresh automation browser".into()
            } else {
                format!("Chrome ({}) — start DevTools browser", self.debug_address())
            },
            platform: Platform::Chrome,
            state: DeviceState::Offline,
        }
    }

    pub(super) async fn start_browser(&self, id: &str) -> Result<(), RobotError> {
        let _guard = self.boot_lock.lock().await;
        if id != "chrome:browser" {
            self.page(id).await?;
            return Ok(());
        }
        if self
            .list_devices()
            .await?
            .iter()
            .any(|d| d.state == DeviceState::Available)
        {
            return Ok(());
        }
        let address = if self.fresh_browser {
            // Reserve a free port, then release it immediately before Chrome starts.
            let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not allocate Chrome DevTools port: {error}"
                ))
            })?;
            listener
                .local_addr()
                .map_err(|error| RobotError::CommandFailed(error.to_string()))?
        } else {
            local_debug_address(&self.debug_address)?
        };
        let profile = fresh_profile()?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create Chrome log directory: {error}"))
            })?;
        let log_path = self
            .artifact_dir
            .join(format!("chrome-startup-{}.log", uuid::Uuid::new_v4()));
        let result = async {
            let log = std::fs::File::create(&log_path).map_err(|error|
                RobotError::CommandFailed(format!("could not create Chrome startup log: {error}")))?;
            let stderr = log.try_clone().map_err(|error| RobotError::CommandFailed(error.to_string()))?;
            let executable = chrome_executable();
            let arguments = vec![
                format!("--remote-debugging-address={}", address.ip()),
                format!("--remote-debugging-port={}", address.port()),
                format!("--user-data-dir={}", profile.display()),
                "--no-first-run".into(), "--no-default-browser-check".into(), "about:blank".into(),
            ];
            #[cfg(target_os = "macos")]
            let native_bundle = app_bundle(std::path::Path::new(&executable));
            #[cfg(not(target_os = "macos"))]
            let native_bundle: Option<PathBuf> = None;
            let mut browser = if let Some(bundle) = native_bundle {
                #[cfg(target_os = "macos")]
                let pid = launch_native(&bundle, &arguments).await?;
                #[cfg(not(target_os = "macos"))]
                let pid = { let _ = bundle; unreachable!() };
                ManagedBrowser { child: None, pid, profile: profile.clone() }
            } else {
                let child = std::process::Command::new(&executable).args(&arguments)
                    .stdin(Stdio::null()).stdout(log).stderr(stderr).spawn()
                    .map_err(|error| RobotError::CommandFailed(format!("could not start Chrome at {executable}: {error}; set DEVICE_BOT_CHROME_PATH")))?;
                ManagedBrowser { pid: child.id(), child: Some(child), profile: profile.clone() }
            };
            tokio::fs::write(log_path.with_extension("json"), json!({
                "pid": browser.pid, "launchMode": if browser.child.is_none() { "launch_services" } else { "direct_process" }, "profile": profile,
                "debugAddress": address.to_string(), "log": log_path,
            }).to_string()).await.map_err(|error|
                RobotError::CommandFailed(format!("could not save Chrome session metadata: {error}")))?;
            *self.managed_address.write().unwrap() = Some(address.to_string());
            let ready = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    if self.targets().await.is_ok_and(|targets| targets.iter().any(|t| t.kind == "page" && t.web_socket_debugger_url.is_some())) {
                        return Ok(());
                    }
                    if !browser.running() {
                        return Err(RobotError::CommandFailed(format!("Chrome exited before DevTools was ready; see {}", log_path.display())));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }).await.map_err(|_| RobotError::CommandFailed(format!(
                "Chrome DevTools at {address} did not become ready; see {}", log_path.display())))?;
            ready?;
            *self.managed_browser.lock().await = Some(browser);
            Ok(())
        }.await;
        if result.is_err() {
            *self.managed_address.write().unwrap() = None;
            let _ = std::fs::remove_dir_all(&profile);
        }
        result
    }
}

fn local_debug_address(value: &str) -> Result<std::net::SocketAddr, RobotError> {
    let value = value
        .strip_prefix("localhost:")
        .map_or_else(|| value.to_owned(), |port| format!("127.0.0.1:{port}"));
    let address: std::net::SocketAddr = value.parse().map_err(|_| {
        RobotError::InvalidInput(
            "Chrome start requires a loopback host:port DevTools address".into(),
        )
    })?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(RobotError::Unsupported(
            "Chrome start requires a loopback DevTools address with a nonzero port".into(),
        ));
    }
    Ok(address)
}

fn chrome_executable() -> String {
    if let Ok(path) = std::env::var("DEVICE_BOT_CHROME_PATH") {
        return path;
    }
    for path in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    ] {
        if std::path::Path::new(path).is_file() {
            return path.into();
        }
    }
    "google-chrome".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_session_does_not_attach_to_an_existing_endpoint() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut robot = ChromeRobot::new(listener.local_addr().unwrap().to_string());
        robot.fresh_browser = true;
        let devices = robot.list_devices().await.unwrap();
        assert_eq!(devices[0].id, "chrome:browser");
        assert_eq!(devices[0].state, DeviceState::Offline);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn every_launch_has_a_new_private_profile() {
        let first = fresh_profile().unwrap();
        let second = fresh_profile().unwrap();
        assert_ne!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(second).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ending_session_kills_only_its_child_and_removes_profile() {
        let profile = fresh_profile().unwrap();
        let child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        drop(ManagedBrowser {
            pid,
            child: Some(child),
            profile: profile.clone(),
        });
        assert!(!profile.exists());
        assert!(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[tokio::test]
    async fn unavailable_endpoint_is_visible_as_stopped_chrome() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let devices = ChromeRobot::new(address.to_string())
            .list_devices()
            .await
            .unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "chrome:browser");
        assert_eq!(devices[0].platform, Platform::Chrome);
        assert_eq!(devices[0].state, DeviceState::Offline);
    }

    #[test]
    fn only_local_endpoints_can_launch_chrome() {
        assert!(local_debug_address("localhost:9222").is_ok());
        assert!(local_debug_address("[::1]:9222").is_ok());
        assert!(local_debug_address("192.168.1.2:9222").is_err());
        assert!(local_debug_address("127.0.0.1:0").is_err());
    }
}

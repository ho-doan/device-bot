use super::*;

impl ChromeRobot {
    pub(super) fn offline_browser(&self) -> RobotDevice {
        RobotDevice {
            id: "chrome:browser".into(),
            name: format!("Chrome ({}) — start DevTools browser", self.debug_address),
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
            .any(|device| device.state == DeviceState::Available)
        {
            return Ok(());
        }
        let address = local_debug_address(&self.debug_address)?;
        let executable = chrome_executable();
        let profile = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".device-bot/chrome")
            .join(address.port().to_string());
        tokio::fs::create_dir_all(&profile).await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not create Chrome automation profile: {error}"
            ))
        })?;
        let log = std::fs::File::create(profile.join("chrome.log")).map_err(|error| {
            RobotError::CommandFailed(format!("could not create Chrome startup log: {error}"))
        })?;
        let stderr = log
            .try_clone()
            .map_err(|error| RobotError::CommandFailed(error.to_string()))?;
        let mut child = Command::new(&executable)
            .arg(format!("--remote-debugging-address={}", address.ip()))
            .arg(format!("--remote-debugging-port={}", address.port()))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check", "about:blank"])
            .stdin(Stdio::null()).stdout(log).stderr(stderr)
            .kill_on_drop(false)
            .spawn().map_err(|error| RobotError::CommandFailed(format!(
                "could not start Chrome at {executable}: {error}; set DEVICE_BOT_CHROME_PATH to the Chrome executable"
            )))?;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if self
                    .list_devices()
                    .await?
                    .iter()
                    .any(|device| device.state == DeviceState::Available)
                {
                    return Ok(());
                }
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| RobotError::CommandFailed(error.to_string()))?
                {
                    return Err(RobotError::CommandFailed(format!(
                        "Chrome exited with {status}; check {}",
                        profile.join("chrome.log").display()
                    )));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|_| {
            RobotError::CommandFailed(format!(
                "Chrome DevTools at {} did not become ready; check {}",
                self.debug_address,
                profile.join("chrome.log").display()
            ))
        })?
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

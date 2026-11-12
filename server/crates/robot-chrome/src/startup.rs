//! Connect to the running Chrome; never launch or terminate the user's browser.
use super::*;

pub(super) fn default_discovery_file() -> PathBuf {
    if let Some(path) = std::env::var_os("DEVICE_BOT_CHROME_ACTIVE_PORT_FILE") {
        return path.into();
    }
    let home = std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Google/Chrome/DevToolsActivePort")
    } else if cfg!(windows) {
        home.join("Google/Chrome/User Data/DevToolsActivePort")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("google-chrome/DevToolsActivePort")
    }
}

fn discovery_endpoint(contents: &str) -> Result<String, RobotError> {
    let mut lines = contents.lines();
    let port = lines
        .next()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or_else(|| {
            RobotError::CommandFailed("invalid Chrome DevToolsActivePort port".into())
        })?;
    let path = lines.next().unwrap_or("").trim();
    if !path.starts_with("/devtools/browser/")
        || path.len() <= "/devtools/browser/".len()
        || !path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-')
    {
        return Err(RobotError::CommandFailed(
            "invalid Chrome DevToolsActivePort browser path".into(),
        ));
    }
    Ok(format!("ws://127.0.0.1:{port}{path}"))
}

impl ChromeRobot {
    pub(super) fn offline_browser(&self) -> RobotDevice {
        RobotDevice {
            id: "chrome:browser".into(),
            name: "Chrome — create private case window in running browser".into(),
            platform: Platform::Chrome,
            state: DeviceState::Offline,
        }
    }

    async fn running_browser_endpoint(&self) -> Result<String, RobotError> {
        if let Some(path) = &self.discovery_file {
            let contents = tokio::fs::read_to_string(path).await.map_err(|_| RobotError::CommandFailed(format!(
                "Chrome remote debugging is not available at {}. Open Chrome, enable chrome://inspect/#remote-debugging, then allow the connection. No replacement browser was launched.", path.display())))?;
            discovery_endpoint(&contents)
        } else {
            let body = tokio::time::timeout(Duration::from_secs(3), self.http_get("/json/version"))
                .await
                .map_err(|_| {
                    RobotError::CommandFailed("running Chrome discovery timed out".into())
                })??;
            let version: Value = serde_json::from_slice(&body).map_err(|e| {
                RobotError::CommandFailed(format!("invalid Chrome version response: {e}"))
            })?;
            version["webSocketDebuggerUrl"]
                .as_str()
                .filter(|s| s.starts_with("ws://") || s.starts_with("wss://"))
                .map(str::to_owned)
                .ok_or_else(|| {
                    RobotError::CommandFailed(
                        "running Chrome has no browser WebSocket endpoint".into(),
                    )
                })
        }
    }

    pub(super) async fn start_browser(&self, id: &str) -> Result<(), RobotError> {
        let _guard = self.boot_lock.lock().await;
        if id != "chrome:browser" {
            self.page(id).await?;
            return Ok(());
        }
        let mut session = self.browser_session.lock().await;
        if let Some(current) = session.as_mut() {
            current.targets().await?;
            return Ok(());
        }
        let connection = {
            let mut shared = self.browser_connection.lock().await;
            if let Some(current) = shared.as_ref() {
                if current.lock().await.poisoned {
                    *shared = None;
                }
            }
            if shared.is_none() {
                let endpoint = self.running_browser_endpoint().await?;
                *shared = Some(Arc::new(Mutex::new(
                    connection::BrowserConnection::connect(&endpoint).await?,
                )));
            }
            Arc::clone(shared.as_ref().unwrap())
        };
        let mut connected = connection::BrowserSession::create(connection).await?;
        let metadata = async {
            tokio::fs::create_dir_all(&self.artifact_dir)
                .await
                .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
            tokio::fs::write(
                self.artifact_dir
                    .join(format!("chrome-context-{}.json", connected.context)),
                json!({"browserContextId":connected.context,"mode":"shared_browser_incognito"})
                    .to_string(),
            )
            .await
            .map_err(|e| {
                RobotError::CommandFailed(format!("could not save Chrome context metadata: {e}"))
            })
        }
        .await;
        if let Err(error) = metadata {
            let _ = connected.close().await;
            return Err(error);
        }
        *session = Some(connected);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_local_discovery_metadata() {
        assert_eq!(
            discovery_endpoint("9222\n/devtools/browser/abc-123\n").unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/abc-123"
        );
        for data in [
            "0\n/devtools/browser/x",
            "9222\nws://other",
            "9222\n/devtools/browser/x?token=bad",
            "bad",
        ] {
            assert!(discovery_endpoint(data).is_err());
        }
    }
    #[tokio::test]
    async fn missing_debugging_returns_setup_error_without_launching_a_browser() {
        let mut robot = ChromeRobot::new("127.0.0.1:0");
        robot.discovery_file =
            Some(std::env::temp_dir().join("device-bot-no-such-active-port-file"));
        assert_eq!(robot.list_devices().await.unwrap()[0].id, "chrome:browser");
        let error = robot
            .boot_device("chrome:browser")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("chrome://inspect/#remote-debugging"));
        assert!(robot.browser_session.lock().await.is_none());
    }
}

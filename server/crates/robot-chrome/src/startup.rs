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
            if let Ok(contents) = tokio::fs::read_to_string(path).await {
                if let Ok(endpoint) = discovery_endpoint(&contents) {
                    return Ok(endpoint);
                }
            }
            // Existing Device Bot Chrome may own 9222 while the personal profile has
            // no DevToolsActivePort. Validate /json/version rather than assuming a collision.
            let body = tokio::time::timeout(Duration::from_secs(3), self.http_get("/json/version")).await
                .map_err(|_| RobotError::CommandFailed("Chrome discovery timed out".into()))?
                .map_err(|_| RobotError::CommandFailed(format!("Chrome remote debugging unavailable at {} and {}. Enable chrome://inspect/#remote-debugging or set DEVICE_BOT_CHROME_DEBUG_ADDRESS. No unrelated process was terminated.", path.display(), self.debug_address)))?;
            let version: Value = serde_json::from_slice(&body)
                .map_err(|e| RobotError::CommandFailed(e.to_string()))?;
            version["webSocketDebuggerUrl"]
                .as_str()
                .filter(|s| s.starts_with("ws://127.0.0.1:") || s.starts_with("ws://localhost:"))
                .map(str::to_owned)
                .ok_or_else(|| {
                    RobotError::CommandFailed(
                        "Occupied endpoint is not a Chrome DevTools browser".into(),
                    )
                })
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
            if current
                .targets()
                .await
                .is_ok_and(|targets| !targets.is_empty())
            {
                return Ok(());
            }
            if let Some(mut stale) = session.take() {
                let _ = stale.close().await;
            }
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
    #[tokio::test]
    async fn missing_profile_file_falls_back_to_valid_debug_endpoint() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut robot = ChromeRobot::new(listener.local_addr().unwrap().to_string());
        robot.discovery_file =
            Some(std::env::temp_dir().join("missing-personal-chrome-profile-port"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            let body = r#"{"webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser/test"}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        assert_eq!(
            robot.running_browser_endpoint().await.unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/test"
        );
        server.await.unwrap();
    }

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

#[cfg(test)]
mod physical_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires a running Chrome CDP endpoint on 9222"]
    async fn real_chrome_missing_profile_start_restart_and_close_case() {
        let mut robot = ChromeRobot::new("127.0.0.1:9222");
        robot.discovery_file =
            Some(std::env::temp_dir().join("device-bot-missing-profile-live-regression"));
        robot.artifact_dir =
            std::env::temp_dir().join(format!("device-bot-chrome-{}", std::process::id()));
        robot.boot_device("chrome:browser").await.unwrap();
        let original = robot.list_devices().await.unwrap()[0].id.clone();
        assert_ne!(original, "chrome:browser");
        robot.restart_device("chrome:browser").await.unwrap();
        assert_ne!(robot.list_devices().await.unwrap()[0].id, original);
        robot.close_session().await.unwrap();
        assert_eq!(robot.list_devices().await.unwrap()[0].id, "chrome:browser");
        println!("Missing profile + occupied Chrome endpoint: boot/restart/close passed");
    }
}

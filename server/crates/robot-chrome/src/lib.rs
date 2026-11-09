//! Chrome driver using the local Chrome DevTools Protocol (CDP).
//!
//! Start Chrome with `--remote-debugging-port=9222`, or point
//! `DEVICE_BOT_CHROME_DEBUG_ADDRESS` at another local DevTools endpoint.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use robot_core::{
    Bounds, DeviceState, ElementType, Platform, Point, Robot, RobotDevice, RobotError, UiElement,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::Command,
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ActiveRecording {
    path: PathBuf,
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), RobotError>>,
}

#[derive(Clone)]
pub struct ChromeRobot {
    debug_address: String,
    artifact_dir: PathBuf,
    ffmpeg_path: String,
    recordings: Arc<Mutex<HashMap<String, ActiveRecording>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChromeTarget {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    web_socket_debugger_url: Option<String>,
}

impl ChromeRobot {
    pub fn new(debug_address: impl Into<String>) -> Self {
        Self {
            debug_address: debug_address.into(),
            artifact_dir: "artifacts".into(),
            ffmpeg_path: "ffmpeg".into(),
            recordings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Self {
        let mut robot = Self::new(
            std::env::var("DEVICE_BOT_CHROME_DEBUG_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:9222".into()),
        );
        robot.artifact_dir = std::env::var("DEVICE_BOT_ARTIFACT_DIR")
            .unwrap_or_else(|_| "artifacts".into())
            .into();
        robot.ffmpeg_path = std::env::var("DEVICE_BOT_FFMPEG_PATH").unwrap_or_else(|_| {
            [
                "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
                "/usr/local/opt/ffmpeg/bin/ffmpeg",
            ]
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
            .map_or_else(|| "ffmpeg".into(), |path| (*path).into())
        });
        robot
    }

    async fn http_get(&self, path: &str) -> Result<Vec<u8>, RobotError> {
        let mut stream = TcpStream::connect(&self.debug_address)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "Chrome DevTools is not reachable at {}: {error}",
                    self.debug_address
                ))
            })?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.debug_address
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not request Chrome DevTools targets: {error}"
                ))
            })?;
        let read_error = |error| {
            RobotError::CommandFailed(format!("could not read Chrome DevTools response: {error}"))
        };
        let mut stream = BufReader::new(stream);
        let mut header = String::new();
        loop {
            if stream.read_line(&mut header).await.map_err(read_error)? == 0 {
                return Err(RobotError::CommandFailed(
                    "Chrome DevTools returned an invalid HTTP response".into(),
                ));
            }
            if header.ends_with("\r\n\r\n") {
                break;
            }
        }
        if !header.starts_with("HTTP/") || !header.contains(" 200 ") {
            return Err(RobotError::CommandFailed(format!(
                "Chrome DevTools request failed: {}",
                header.lines().next().unwrap_or("invalid response")
            )));
        }
        let content_length = header
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, length)| length.trim().parse::<u64>())
            .transpose()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "Chrome DevTools returned an invalid Content-Length: {error}"
                ))
            })?;
        let mut body = vec![];
        if let Some(length) = content_length {
            stream
                .take(length)
                .read_to_end(&mut body)
                .await
                .map_err(read_error)?;
            if body.len() as u64 != length {
                return Err(RobotError::CommandFailed(
                    "Chrome DevTools returned a truncated HTTP body".into(),
                ));
            }
        } else {
            stream.read_to_end(&mut body).await.map_err(read_error)?;
        }
        Ok(body)
    }

    async fn targets(&self) -> Result<Vec<ChromeTarget>, RobotError> {
        let response = self.http_get("/json/list").await?;
        serde_json::from_slice(&response).map_err(|error| {
            RobotError::CommandFailed(format!(
                "Chrome DevTools returned invalid target JSON: {error}"
            ))
        })
    }

    async fn page(&self, device_id: &str) -> Result<ChromeTarget, RobotError> {
        let target_id = device_id
            .strip_prefix("chrome:")
            .ok_or_else(|| RobotError::DeviceNotFound(device_id.into()))?;
        self.targets()
            .await?
            .into_iter()
            .find(|target| {
                target.kind == "page"
                    && target.id == target_id
                    && target.web_socket_debugger_url.is_some()
            })
            .ok_or_else(|| RobotError::DeviceNotFound(device_id.into()))
    }

    async fn cdp(
        &self,
        target: &ChromeTarget,
        method: &str,
        params: Value,
    ) -> Result<Value, RobotError> {
        let endpoint = target.web_socket_debugger_url.as_deref().ok_or_else(|| {
            RobotError::CommandFailed(format!(
                "Chrome target {} has no debugger endpoint",
                target.id
            ))
        })?;
        let (mut socket, _) = connect_async(endpoint).await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not connect to Chrome target {}: {error}",
                target.id
            ))
        })?;
        let id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not send Chrome command {method}: {error}"
                ))
            })?;
        while let Some(message) = socket.next().await {
            let message = message.map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not read Chrome command {method}: {error}"
                ))
            })?;
            if !message.is_text() {
                continue;
            }
            let value: Value = serde_json::from_str(&message.into_text().map_err(|error| {
                RobotError::CommandFailed(format!("Chrome returned invalid text frame: {error}"))
            })?)
            .map_err(|error| {
                RobotError::CommandFailed(format!("Chrome returned invalid JSON: {error}"))
            })?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(RobotError::CommandFailed(format!(
                    "Chrome command {method} failed: {error}"
                )));
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(RobotError::CommandFailed(format!(
            "Chrome closed the command channel while running {method}"
        )))
    }

    async fn dispatch_mouse(
        &self,
        target: &ChromeTarget,
        event_type: &str,
        point: Point,
        button: Option<&str>,
    ) -> Result<(), RobotError> {
        let mut params = json!({ "type": event_type, "x": point.x, "y": point.y });
        if let Some(button) = button {
            params["button"] = Value::String(button.into());
            params["clickCount"] = json!(1);
        }
        self.cdp(target, "Input.dispatchMouseEvent", params)
            .await
            .map(|_| ())
    }

    async fn capture_screenshot(&self, target: &ChromeTarget) -> Result<Vec<u8>, RobotError> {
        let result = self
            .cdp(target, "Page.captureScreenshot", json!({ "format": "png" }))
            .await?;
        let data = result.get("data").and_then(Value::as_str).ok_or_else(|| {
            RobotError::CommandFailed("Chrome did not return screenshot data".into())
        })?;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "Chrome returned invalid screenshot data: {error}"
                ))
            })
    }
}

fn device_id(target: &ChromeTarget) -> String {
    format!("chrome:{}", target.id)
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn value_i32(value: &Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_default()
        .round() as i32
}

#[async_trait]
impl Robot for ChromeRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        // Chrome is optional. An inactive DevTools endpoint must not stop the
        // mobile and desktop drivers from listing their own devices.
        let targets = match self.targets().await {
            Ok(targets) => targets,
            Err(_) => return Ok(vec![]),
        };
        Ok(targets
            .into_iter()
            .filter(|target| target.kind == "page" && target.web_socket_debugger_url.is_some())
            .map(|target| RobotDevice {
                id: device_id(&target),
                name: if target.title.is_empty() {
                    target.url
                } else {
                    target.title
                },
                platform: Platform::Chrome,
                state: DeviceState::Available,
            })
            .collect())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        let target = self.page(device_id).await?;
        self.capture_screenshot(&target).await
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        let target = self.page(device_id).await?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create Chrome recording directory: {error}"
                ))
            })?;
        let path = self.artifact_dir.join(format!(
            "chrome-recording-{}.mp4",
            RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut recordings = self.recordings.lock().await;
        if recordings
            .get(device_id)
            .is_some_and(|recording| !recording.task.is_finished())
        {
            return Err(RobotError::InvalidInput(format!(
                "a Chrome recording is already in progress for {device_id}"
            )));
        }
        recordings.remove(device_id);
        let mut encoder = Command::new(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "image2pipe",
                "-framerate",
                "10",
                "-vcodec",
                "png",
                "-i",
                "-",
                "-an",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start ffmpeg for Chrome recording (set DEVICE_BOT_FFMPEG_PATH if needed): {error}"
                ))
            })?;
        let mut stdin = encoder.stdin.take().ok_or_else(|| {
            RobotError::CommandFailed("ffmpeg did not provide a writable video input".into())
        })?;
        let (stop, mut stopped) = oneshot::channel();
        let robot = self.clone();
        let task = tokio::spawn(async move {
            let mut frames = interval(Duration::from_millis(100));
            frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut capture_error = None;
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    _ = frames.tick() => match robot.capture_screenshot(&target).await {
                        Ok(frame) => if let Err(error) = stdin.write_all(&frame).await {
                            capture_error = Some(RobotError::CommandFailed(format!("could not send a Chrome frame to ffmpeg: {error}")));
                            break;
                        },
                        Err(error) => {
                            capture_error = Some(error);
                            break;
                        }
                    },
                }
            }
            drop(stdin);
            let output = encoder.wait_with_output().await.map_err(|error| {
                RobotError::CommandFailed(format!("could not finalize Chrome recording: {error}"))
            })?;
            if let Some(error) = capture_error {
                return Err(error);
            }
            if !output.status.success() {
                return Err(RobotError::CommandFailed(format!(
                    "ffmpeg could not encode Chrome recording: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            Ok(())
        });
        recordings.insert(
            device_id.into(),
            ActiveRecording {
                path: path.clone(),
                stop,
                task,
            },
        );
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.page(device_id).await?;
        let recording = self
            .recordings
            .lock()
            .await
            .remove(device_id)
            .ok_or_else(|| {
                RobotError::InvalidInput(format!(
                    "no Chrome recording is in progress for {device_id}"
                ))
            })?;
        let _ = recording.stop.send(());
        recording.task.await.map_err(|error| {
            RobotError::CommandFailed(format!("Chrome recording task failed: {error}"))
        })??;
        if !recording.path.is_file() {
            return Err(RobotError::CommandFailed(
                "ffmpeg did not create a Chrome recording artifact".into(),
            ));
        }
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.dispatch_mouse(&target, "mousePressed", point, Some("left"))
            .await?;
        self.dispatch_mouse(&target, "mouseReleased", point, Some("left"))
            .await
    }

    async fn launch_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "Chrome pages cannot launch native applications".into(),
        ))
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        _app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.cdp(&target, "Page.navigate", json!({ "url": url }))
            .await
            .map(|_| ())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.cdp(&target, "Input.insertText", json!({ "text": text }))
            .await
            .map(|_| ())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        _duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        let target = self.page(device_id).await?;
        self.dispatch_mouse(&target, "mousePressed", start, Some("left"))
            .await?;
        for step in 1..=12 {
            let fraction = f64::from(step) / 12.0;
            self.dispatch_mouse(
                &target,
                "mouseMoved",
                Point {
                    x: start.x + (end.x - start.x) * fraction,
                    y: start.y + (end.y - start.y) * fraction,
                },
                None,
            )
            .await?;
        }
        self.dispatch_mouse(&target, "mouseReleased", end, Some("left"))
            .await
    }

    async fn install_app(&self, device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "app installation is not supported by the Chrome robot".into(),
        ))
    }

    async fn uninstall_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.page(device_id).await?;
        Err(RobotError::Unsupported(
            "app removal is not supported by the Chrome robot".into(),
        ))
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        let target = self.page(device_id).await?;
        let expression = include_str!("dom-elements.js");
        let result = self
            .cdp(
                &target,
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                }),
            )
            .await?;
        let values = result
            .pointer("/result/value")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                RobotError::CommandFailed("Chrome did not return DOM element data".into())
            })?;
        Ok(values
            .iter()
            .map(|value| UiElement {
                class_name: value_string(value, "className").unwrap_or_else(|| "element".into()),
                resource_id: value_string(value, "resourceId"),
                text: value_string(value, "text"),
                content_description: value_string(value, "contentDescription"),
                bounds: Bounds {
                    left: value_i32(value, "left"),
                    top: value_i32(value, "top"),
                    right: value_i32(value, "right"),
                    bottom: value_i32(value, "bottom"),
                },
                clickable: value
                    .get("clickable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                enabled: value
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                focused: value.get("focused").and_then(Value::as_bool),
                checked: value.get("checked").and_then(Value::as_bool),
                selected: value.get("selected").and_then(Value::as_bool),
                value: value
                    .get("value")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                element_type: value
                    .get("elementType")
                    .and_then(Value::as_str)
                    .and_then(|element_type| ElementType::parse(element_type).ok()),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn serve_http(response: Vec<u8>, keep_open: bool) -> (ChromeRobot, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let robot = ChromeRobot::new(listener.local_addr().unwrap().to_string());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![];
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }
            // Fragment both headers and body to exercise TCP framing.
            for chunk in response.chunks(7) {
                stream.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
            if keep_open {
                std::future::pending::<()>().await;
            }
        });
        (robot, task)
    }

    #[tokio::test]
    async fn discovery_finishes_when_body_arrives_before_connection_closes() {
        let body = br#"[{"id":"A1B2","type":"page","title":"fixture","url":"about:blank","webSocketDebuggerUrl":"ws://localhost/page/A1B2"}]"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncOnTeNt-LeNgTh: {}\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let (robot, server) = serve_http(response.into_bytes(), true).await;
        let result = tokio::time::timeout(Duration::from_secs(1), robot.list_devices()).await;
        server.abort();
        let devices = result.expect("discovery waited for EOF").unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "chrome:A1B2");
    }

    #[tokio::test]
    async fn http_rejects_truncated_body() {
        let (robot, server) = serve_http(
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n[]".to_vec(),
            false,
        )
        .await;
        assert!(robot.http_get("/json/list").await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_without_content_length_reads_until_close() {
        let (robot, server) = serve_http(b"HTTP/1.0 200 OK\r\n\r\n[]".to_vec(), false).await;
        assert_eq!(robot.http_get("/json/list").await.unwrap(), b"[]");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn list_elements_preserves_empty_field_value() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let body = json!([{"id":"A1B2","type":"page","webSocketDebuggerUrl":endpoint}]).to_string();
        let (robot, http) = serve_http(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
            false,
        )
        .await;
        let ws = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request: Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(request["method"], "Runtime.evaluate");
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":{"result":{"value":[
                        {"className":"input","value":"","elementType":"textField"},
                        {"className":"input","value":"hello"},
                        {"className":"div","value":null}
                    ]}}})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });
        let elements = robot.list_elements("chrome:A1B2").await.unwrap();
        assert_eq!(elements[0].value.as_deref(), Some(""));
        assert_eq!(elements[1].value.as_deref(), Some("hello"));
        assert_eq!(elements[2].value, None);
        assert_eq!(elements[0].resource_id, None);
        ws.await.unwrap();
        http.await.unwrap();
    }

    #[test]
    fn chrome_target_becomes_a_device_id() {
        let target = ChromeTarget {
            id: "A1B2".into(),
            title: String::new(),
            url: String::new(),
            kind: "page".into(),
            web_socket_debugger_url: Some("ws://localhost/devtools/page/A1B2".into()),
        };
        assert_eq!(device_id(&target), "chrome:A1B2");
    }
}

//! Chrome driver using the local Chrome DevTools Protocol (CDP).
//!
//! Start automatically enables loopback debugging in a Device Bot profile when
//! no running Chrome debugger is available. `DEVICE_BOT_CHROME_DEBUG_ADDRESS`
//! explicitly selects an existing DevTools endpoint instead.

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

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use robot_core::{DeviceState, Platform, Point, RobotDevice, RobotError, WebContext};
#[cfg(test)]
use robot_core::{ElementQuery, Robot, UiElement};
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

mod adapter;
mod connection;
mod dom;
mod ffmpeg;
mod frames;
#[cfg(test)]
#[path = "tests/iframe_smoke_tests.rs"]
mod iframe_smoke_tests;
mod live;
mod managed;
mod observation;
#[cfg(test)]
#[path = "tests/qc_smoke_tests.rs"]
mod qc_smoke_tests;
mod screencast;
mod startup;
mod test_proxy;

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
    observers: Arc<Mutex<HashMap<String, observation::Observer>>>,
    boot_lock: Arc<Mutex<()>>,
    drag_lock: Arc<Mutex<()>>,
    browser_session: Arc<Mutex<Option<connection::BrowserSession>>>,
    browser_connection: Arc<Mutex<Option<Arc<Mutex<connection::BrowserConnection>>>>>,
    test_proxy: Arc<Mutex<Option<test_proxy::TestProxy>>>,
    discovery_file: Option<PathBuf>,
    managed_browser: Option<Arc<Mutex<managed::ManagedChrome>>>,
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
            observers: Arc::default(),
            boot_lock: Arc::default(),
            drag_lock: Arc::default(),
            browser_session: Arc::default(),
            browser_connection: Arc::default(),
            test_proxy: Arc::default(),
            discovery_file: None,
            managed_browser: None,
        }
    }

    pub fn from_env() -> Self {
        let explicit_address = std::env::var("DEVICE_BOT_CHROME_DEBUG_ADDRESS").ok();
        let mut robot = Self::new(
            explicit_address
                .clone()
                .unwrap_or_else(|| "127.0.0.1:9222".into()),
        );
        if explicit_address.is_none() {
            robot.discovery_file = Some(startup::default_discovery_file());
        }
        robot.artifact_dir = std::env::var("DEVICE_BOT_ARTIFACT_DIR")
            .unwrap_or_else(|_| "artifacts".into())
            .into();
        if explicit_address.is_none()
            && std::env::var_os("DEVICE_BOT_CHROME_ACTIVE_PORT_FILE").is_none()
        {
            robot.managed_browser = Some(Arc::new(Mutex::new(managed::ManagedChrome::new(
                robot.artifact_dir.join("chrome-profile"),
                std::env::var_os("DEVICE_BOT_CHROME_PATH").map(PathBuf::from),
            ))));
        }
        robot.ffmpeg_path = ffmpeg::resolve(
            std::env::var("DEVICE_BOT_FFMPEG_PATH").ok(),
            std::env::current_exe().ok().as_deref(),
            &[
                "/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg",
                "/usr/local/opt/ffmpeg-full/bin/ffmpeg",
                "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
                "/usr/local/opt/ffmpeg/bin/ffmpeg",
            ],
        );
        robot
    }

    async fn http_get(&self, path: &str) -> Result<Vec<u8>, RobotError> {
        let address = self.debug_address.clone();
        let mut stream = TcpStream::connect(&address).await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "Chrome DevTools is not reachable at {}: {error}",
                address
            ))
        })?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            address
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
        let mut session = self.browser_session.lock().await;
        if let Some(session) = session.as_mut() {
            return session.targets().await;
        }
        Err(RobotError::CommandFailed(
            "Chrome case context is not started; call mobile_boot_device with chrome:browser"
                .into(),
        ))
    }

    #[cfg(test)]
    async fn endpoint_targets(&self) -> Result<Vec<ChromeTarget>, RobotError> {
        let response = tokio::time::timeout(Duration::from_secs(2), self.http_get("/json/list"))
            .await
            .map_err(|_| {
                RobotError::CommandFailed("Chrome DevTools discovery timed out".into())
            })??;
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
        let mut session = self.browser_session.lock().await;
        if let Some(session) = session.as_mut() {
            return session.page_request(target, method, params).await;
        }
        tokio::time::timeout(
            Duration::from_secs(5),
            self.cdp_request(target, method, params),
        )
        .await
        .map_err(|_| {
            RobotError::CommandFailed(format!(
                "Chrome command {method} timed out after 5 seconds on target {}",
                target.id
            ))
        })?
    }

    async fn cdp_many(
        &self,
        target: &ChromeTarget,
        method: &str,
        params: Vec<Value>,
    ) -> Result<Vec<Value>, RobotError> {
        let mut session = self.browser_session.lock().await;
        if let Some(session) = session.as_mut() {
            return session.page_requests(target, method, params).await;
        }
        let endpoint = target.web_socket_debugger_url.as_deref().ok_or_else(|| {
            RobotError::CommandFailed(format!(
                "Chrome target {} has no debugger endpoint",
                target.id
            ))
        })?;
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut connection = connection::BrowserConnection::connect(endpoint).await?;
            connection.request_many(method, params, None).await
        })
        .await
        .map_err(|_| {
            RobotError::CommandFailed(format!(
                "Chrome command {method} timed out after 5 seconds on target {}",
                target.id
            ))
        })?
    }

    async fn cdp_request(
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

    async fn checked_pointer_points(
        &self,
        target: &ChromeTarget,
        context: &WebContext,
        fractions: &[[f64; 2]],
    ) -> Result<Vec<Point>, RobotError> {
        let reference = context.element_ref.as_ref().ok_or_else(|| {
            RobotError::InvalidInput(
                "Inspect the DOM element again before sending the gesture".into(),
            )
        })?;
        let script = format!(
            "({})(document, {}, {}, {})",
            include_str!("dom-pointer-point.js"),
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            serde_json::to_string(reference).unwrap(),
            json!(fractions),
        );
        let (_, mapping) = self
            .eval_in_context_value(target, context, &script, true)
            .await?;
        if mapping.len() != fractions.len() {
            return Err(RobotError::InvalidInput(
                "Incomplete DOM pointer mapping".into(),
            ));
        }
        Ok(mapping)
    }

    async fn dispatch_pointer_gesture(
        &self,
        target: &ChromeTarget,
        point: Point,
        gesture: robot_core::PointerGesture,
    ) -> Result<(), RobotError> {
        use robot_core::PointerGesture::*;
        match gesture {
            Tap => self.dispatch_tap(target, point).await,
            LongPress { duration_ms } => {
                // Chrome owns both ends of this gesture, even if the MCP caller times out.
                self.cdp(
                    target,
                    "Input.synthesizeTapGesture",
                    json!({
                        "x": point.x, "y": point.y, "duration": duration_ms,
                        "tapCount": 1, "gestureSourceType": "mouse",
                    }),
                )
                .await
                .map(|_| ())
            }
            DoubleTap => {
                self.dispatch_mouse(target, "mouseMoved", point, None)
                    .await?;
                let mut events = Vec::with_capacity(4);
                for count in [1, 2] {
                    for event in ["mousePressed", "mouseReleased"] {
                        let mut params = Self::mouse_event_params(event, point, Some("left"));
                        params["clickCount"] = json!(count);
                        events.push(params);
                    }
                }
                self.cdp_many(target, "Input.dispatchMouseEvent", events)
                    .await
                    .map(|_| ())
            }
        }
    }

    async fn dispatch_touch_drag(
        &self,
        target: &ChromeTarget,
        start: Point,
        end: Point,
        duration: u64,
    ) -> Result<(), RobotError> {
        const METHOD: &str = "Input.dispatchTouchEvent";
        let at = |kind, x, y| json!({"type":kind,"touchPoints":[{"x":x,"y":y,"id":0}]});
        let motion = async {
            self.cdp(target, METHOD, at("touchStart", start.x, start.y))
                .await?;
            const STEPS: u64 = 12;
            let started = tokio::time::Instant::now();
            for step in 1..=STEPS {
                let t = step as f64 / STEPS as f64;
                tokio::time::sleep_until(started + Duration::from_millis(duration * step / STEPS))
                    .await;
                self.cdp(
                    target,
                    METHOD,
                    at(
                        "touchMove",
                        start.x + (end.x - start.x) * t,
                        start.y + (end.y - start.y) * t,
                    ),
                )
                .await?;
            }
            Ok(())
        }
        .await;
        let kind = if motion.is_ok() {
            "touchEnd"
        } else {
            "touchCancel"
        };
        let release = self
            .cdp(target, METHOD, json!({"type":kind,"touchPoints":[]}))
            .await;
        if motion.is_ok() && release.is_err() {
            let _ = self
                .cdp(
                    target,
                    METHOD,
                    json!({"type":"touchCancel","touchPoints":[]}),
                )
                .await;
        }
        motion.and(release.map(|_| ()))
    }

    async fn dispatch_tap(&self, target: &ChromeTarget, point: Point) -> Result<(), RobotError> {
        self.cdp(target, "Page.bringToFront", json!({})).await?;
        self.dispatch_mouse(target, "mouseMoved", point, None)
            .await?;
        // Queue release before awaiting press: a native popup can defer the
        // press acknowledgement until it receives the matching mouseup.
        self.cdp_many(
            target,
            "Input.dispatchMouseEvent",
            vec![
                Self::mouse_event_params("mousePressed", point, Some("left")),
                Self::mouse_event_params("mouseReleased", point, Some("left")),
            ],
        )
        .await
        .map(|_| ())
    }

    async fn dispatch_mouse(
        &self,
        target: &ChromeTarget,
        event_type: &str,
        point: Point,
        button: Option<&str>,
    ) -> Result<(), RobotError> {
        self.cdp(
            target,
            "Input.dispatchMouseEvent",
            Self::mouse_event_params(event_type, point, button),
        )
        .await
        .map(|_| ())
    }

    fn mouse_event_params(event_type: &str, point: Point, button: Option<&str>) -> Value {
        let mut params = json!({ "type": event_type, "x": point.x, "y": point.y,
            "buttons": if button.is_some() && event_type != "mouseReleased" { 1 } else { 0 } });
        if let Some(button) = button {
            params["button"] = Value::String(button.into());
            // Flutter Web checks PointerEvent.buttons, not only button/clickCount.
            // CDP defaults the held-button bitmask to zero, even on mousePressed.
            params["buttons"] = json!(if event_type == "mouseReleased" { 0 } else { 1 });
            params["clickCount"] = json!(1);
        }
        params
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

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests/connection_tests.rs"]
mod connection_tests;

#[cfg(test)]
#[path = "tests/frame_discovery_tests.rs"]
mod frame_discovery_tests;

//! Android live inspection metadata and input. Video uses the WebRTC media gateway.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex as StdMutex},
    time::Duration,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use robot_core::{DeviceState, Platform, Point, Robot, RobotError, UiElement};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{AdminState, ErrorResponse, is_authorized};

const ELEMENT_GAP: Duration = Duration::from_millis(1200);

pub(crate) async fn android_live(
    Path(device_id): Path<String>,
    Query(query): Query<LiveQuery>,
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(max_frames) = query.max_frames {
        if !(1..=30).contains(&max_frames) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "maxFrames must be between 1 and 30".into(),
                }),
            )
                .into_response();
        }
    }
    if let Err(response) = require_android(&state, &device_id).await {
        return response;
    }
    state
        .activity_log
        .record("admin.live", device_id.clone())
        .await;

    let (sender, receiver) = mpsc::channel(4);
    let robot = Arc::clone(&state.robot);
    let max_frames = query.max_frames;
    tokio::spawn(async move {
        mirror(robot, device_id, sender, max_frames).await;
    });
    Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

pub(crate) async fn android_input(
    Path(device_id): Path<String>,
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(input): Json<LiveInput>,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(response) = require_android(&state, &device_id).await {
        return response;
    }
    let outcome = {
        let gate = device_gate(&device_id);
        let _guard = gate.lock().await;
        dispatch_input(state.robot.as_ref(), &device_id, &input).await
    };
    match outcome {
        Ok(detail) => {
            state.activity_log.record("admin.input", detail).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            let status = match &error {
                RobotError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                RobotError::DeviceNotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::BAD_GATEWAY,
            };
            (
                status,
                Json(ErrorResponse {
                    error: error.to_string(),
                }),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveQuery {
    max_frames: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveInput {
    action: String,
    x: Option<f64>,
    y: Option<f64>,
    end_x: Option<f64>,
    end_y: Option<f64>,
    text: Option<String>,
    duration_ms: Option<u64>,
    delta_x: Option<f64>,
    delta_y: Option<f64>,
}

async fn require_android(state: &AdminState, device_id: &str) -> Result<(), Response> {
    let devices = match state.robot.list_devices().await {
        Ok(devices) => devices,
        Err(error) => {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: error.to_string(),
                }),
            )
                .into_response());
        }
    };
    match devices.iter().find(|device| device.id == device_id) {
        Some(device)
            if device.platform == Platform::Android && device.state == DeviceState::Available =>
        {
            Ok(())
        }
        Some(device) if device.platform != Platform::Android => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Live mirror and live control are available for Android devices.".into(),
            }),
        )
            .into_response()),
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "The Android device is not available.".into(),
            }),
        )
            .into_response()),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("device not found: {device_id}"),
            }),
        )
            .into_response()),
    }
}

async fn dispatch_input(
    robot: &dyn Robot,
    device_id: &str,
    input: &LiveInput,
) -> Result<String, RobotError> {
    match input.action.as_str() {
        "tap" => {
            let point = required_point(input.x, input.y)?;
            robot.tap(device_id, point).await?;
            Ok(format!(
                "{device_id} tap {},{}",
                point.x.round(),
                point.y.round()
            ))
        }
        "swipe" => {
            let start = required_point(input.x, input.y)?;
            let end = required_point(input.end_x, input.end_y)?;
            let duration_ms = input.duration_ms.unwrap_or(300);
            if !(1..=60_000).contains(&duration_ms) {
                return Err(RobotError::InvalidInput(
                    "durationMs must be between 1 and 60000".into(),
                ));
            }
            robot
                .swipe(device_id, start, end, Some(duration_ms))
                .await?;
            Ok(format!(
                "{device_id} swipe {},{} -> {},{}",
                start.x.round(),
                start.y.round(),
                end.x.round(),
                end.y.round()
            ))
        }
        "scroll" => {
            let at = required_point(input.x, input.y)?;
            let dx = input.delta_x.unwrap_or(0.);
            let dy = input.delta_y.unwrap_or(0.);
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 10000. || dy.abs() > 10000. {
                return Err(RobotError::InvalidInput(
                    "Invalid pixel scroll delta".into(),
                ));
            }
            robot.scroll(device_id, at, dx, dy).await?;
            Ok(format!("{device_id} scroll {dx},{dy}"))
        }
        "clear" => {
            robot.clear_text(device_id).await?;
            Ok(format!("{device_id} clear text"))
        }
        "type" => {
            let text = input.text.as_deref().unwrap_or("").trim();
            if text.is_empty() {
                return Err(RobotError::InvalidInput("text is required for type".into()));
            }
            if text.chars().count() > 500 {
                return Err(RobotError::InvalidInput(
                    "text must be 500 characters or fewer".into(),
                ));
            }
            robot.type_text(device_id, text).await?;
            Ok(format!("{device_id} type"))
        }
        _ => Err(RobotError::InvalidInput(
            "action must be tap, swipe, scroll, type, or clear".into(),
        )),
    }
}

fn required_point(x: Option<f64>, y: Option<f64>) -> Result<Point, RobotError> {
    let (Some(x), Some(y)) = (x, y) else {
        return Err(RobotError::InvalidInput(
            "x and y coordinates are required".into(),
        ));
    };
    if !x.is_finite()
        || !y.is_finite()
        || !(0.0..=100_000.0).contains(&x)
        || !(0.0..=100_000.0).contains(&y)
    {
        return Err(RobotError::InvalidInput(
            "coordinates must be finite screen pixels".into(),
        ));
    }
    Ok(Point { x, y })
}

fn device_gate(device_id: &str) -> Arc<Mutex<()>> {
    static GATES: LazyLock<StdMutex<HashMap<String, Arc<Mutex<()>>>>> =
        LazyLock::new(|| StdMutex::new(HashMap::new()));
    let mut gates = GATES.lock().unwrap_or_else(|poison| poison.into_inner());
    gates
        .entry(device_id.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreferredTree {
    Auto,
    Flutter,
}

async fn mirror(
    robot: Arc<dyn Robot>,
    device_id: String,
    sender: mpsc::Sender<Result<Event, std::convert::Infallible>>,
    max_frames: Option<u32>,
) {
    let mut updates = 0;
    let mut preferred = PreferredTree::Auto;
    loop {
        let listed = tokio::select! {
            _ = sender.closed() => break,
            listed = async {
                let gate = device_gate(&device_id);
                let _guard = gate.lock().await;
                inspection_tree(robot.as_ref(), &device_id, &mut preferred).await
            } => listed,
        };
        let sent = match listed {
            Ok((elements, source)) => {
                send_json(
                    &sender,
                    "elements",
                    json!({"elements":elements,"source":source}),
                )
                .await
            }
            Err(error) => send_json(&sender, "error", json!({"error":error.to_string()})).await,
        };
        updates += 1;
        if sent.is_err() || max_frames.is_some_and(|limit| updates >= limit) {
            break;
        }
        tokio::select! { _ = sender.closed() => break, _ = tokio::time::sleep(ELEMENT_GAP) => {} }
    }
}

async fn inspection_tree(
    robot: &dyn Robot,
    device_id: &str,
    preferred: &mut PreferredTree,
) -> Result<(Vec<UiElement>, &'static str), RobotError> {
    if *preferred == PreferredTree::Flutter {
        match robot.list_flutter_elements(device_id).await {
            Ok(mut elements) if !elements.is_empty() => {
                robot_core::prepare_elements(&mut elements);
                return Ok((elements, "flutter"));
            }
            Ok(_) => *preferred = PreferredTree::Auto,
            Err(error) => {
                *preferred = PreferredTree::Auto;
                return Err(error);
            }
        }
    }

    let mut elements = robot.list_elements(device_id).await?;
    let mut source = "native";
    if elements.is_empty() || elements.iter().any(|element| !has_label(element)) {
        if let Ok(flutter) = robot.list_flutter_elements(device_id).await {
            if !flutter.is_empty() {
                elements = flutter;
                source = "flutter";
                *preferred = PreferredTree::Flutter;
            }
        }
    }
    robot_core::prepare_elements(&mut elements);
    Ok((elements, source))
}

fn has_label(element: &UiElement) -> bool {
    let present =
        |value: &Option<String>| value.as_deref().is_some_and(|text| !text.trim().is_empty());
    present(&element.text) || present(&element.content_description)
}

async fn send_json(
    sender: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    event: &str,
    payload: serde_json::Value,
) -> Result<(), mpsc::error::SendError<Result<Event, std::convert::Infallible>>> {
    sender
        .send(Ok(Event::default()
            .event(event)
            .json_data(payload)
            .expect("live event payload is JSON")))
        .await
}

#[cfg(test)]
mod tests {
    use super::{LiveInput, LiveQuery, android_input, android_live};
    use crate::{ActivityLog, AdminState};
    use async_trait::async_trait;
    use axum::{
        Json,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode, header},
    };
    use robot_core::{
        Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement,
    };
    use std::sync::{Arc, Mutex as StdMutex};

    struct LiveRobot {
        actions: Arc<StdMutex<Vec<String>>>,
        flutter: bool,
    }

    #[async_trait]
    impl Robot for LiveRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(vec![
                RobotDevice {
                    id: "phone".into(),
                    name: "Phone".into(),
                    platform: Platform::Android,
                    state: DeviceState::Available,
                },
                RobotDevice {
                    id: "offline".into(),
                    name: "Offline".into(),
                    platform: Platform::Android,
                    state: DeviceState::Offline,
                },
                RobotDevice {
                    id: "browser".into(),
                    name: "Chrome".into(),
                    platform: Platform::Chrome,
                    state: DeviceState::Available,
                },
            ])
        }

        async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
            panic!("live inspection must not capture screenshots: {device_id}")
        }

        async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
            self.actions
                .lock()
                .unwrap()
                .push(format!("tap {device_id} {},{}", point.x, point.y));
            Ok(())
        }

        async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("unused".into()))
        }

        async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("unused".into()))
        }

        async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
            self.actions
                .lock()
                .unwrap()
                .push(format!("clear {device_id}"));
            Ok(())
        }
        async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
            self.actions
                .lock()
                .unwrap()
                .push(format!("type {device_id} {text}"));
            Ok(())
        }

        async fn swipe(
            &self,
            device_id: &str,
            start: Point,
            end: Point,
            duration_ms: Option<u64>,
        ) -> Result<(), RobotError> {
            self.actions.lock().unwrap().push(format!(
                "swipe {device_id} {},{} {},{} {}",
                start.x,
                start.y,
                end.x,
                end.y,
                duration_ms.unwrap_or(0)
            ));
            Ok(())
        }

        async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("unused".into()))
        }

        async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("unused".into()))
        }

        async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
            if device_id != "phone" {
                return Err(RobotError::DeviceNotFound(device_id.to_owned()));
            }
            Ok(vec![sample_element("Save")])
        }

        async fn list_flutter_elements(
            &self,
            _device_id: &str,
        ) -> Result<Vec<UiElement>, RobotError> {
            if !self.flutter {
                return Ok(Vec::new());
            }
            Ok(vec![sample_element("Flutter Save")])
        }
    }

    fn sample_element(text: &str) -> UiElement {
        UiElement {
            class_name: "Button".into(),
            resource_id: Some("save-button".into()),
            text: Some(text.into()),
            content_description: None,
            bounds: Bounds {
                left: 10,
                top: 20,
                right: 110,
                bottom: 60,
            },
            clickable: true,
            enabled: true,
            focused: None,
            checked: None,
            selected: None,
            value: None,
            element_type: None,
            web_context: None,
        }
    }

    fn state(flutter: bool) -> (AdminState, Arc<StdMutex<Vec<String>>>) {
        let actions = Arc::new(StdMutex::new(Vec::new()));
        let admin = AdminState::new(
            Arc::new(LiveRobot {
                actions: Arc::clone(&actions),
                flutter,
            }),
            ActivityLog::new(8),
            Some("test-token".into()),
        );
        (admin, actions)
    }

    fn authorized() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn clear_text_uses_the_robot_boundary_without_typing_an_empty_string() {
        let (admin, actions) = state(false);
        let response = android_input(
            Path("phone".into()),
            State(admin),
            authorized(),
            Json(LiveInput {
                action: "clear".into(),
                x: None,
                y: None,
                end_x: None,
                end_y: None,
                text: None,
                duration_ms: None,
                delta_x: None,
                delta_y: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(*actions.lock().unwrap(), vec!["clear phone"]);
    }

    #[tokio::test]
    async fn live_inspection_emits_elements_without_capturing_screenshots() {
        let (admin, _) = state(false);
        let response = android_live(
            Path("phone".into()),
            Query(LiveQuery {
                max_frames: Some(1),
            }),
            State(admin),
            authorized(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!text.contains("event: frame"), "{text}");
        assert!(text.contains("event: elements"), "{text}");
        assert!(text.contains("Save"), "{text}");
        assert!(text.contains("\"source\":\"native\""), "{text}");
    }

    #[tokio::test]
    async fn live_stream_uses_flutter_elements_when_native_nodes_have_no_label() {
        let actions = Arc::new(StdMutex::new(Vec::new()));
        let admin = AdminState::new(
            Arc::new(UnlabeledNative {
                inner: LiveRobot {
                    actions,
                    flutter: true,
                },
            }),
            ActivityLog::new(8),
            Some("test-token".into()),
        );
        let response = android_live(
            Path("phone".into()),
            Query(LiveQuery {
                max_frames: Some(1),
            }),
            State(admin),
            authorized(),
        )
        .await;
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Flutter Save"), "{text}");
        assert!(text.contains("\"source\":\"flutter\""), "{text}");
    }

    struct UnlabeledNative {
        inner: LiveRobot,
    }

    #[async_trait]
    impl Robot for UnlabeledNative {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            self.inner.list_devices().await
        }
        async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
            self.inner.take_screenshot(device_id).await
        }
        async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
            self.inner.tap(device_id, point).await
        }
        async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
            self.inner.launch_app(device_id, app_id).await
        }
        async fn open_url(
            &self,
            device_id: &str,
            url: &str,
            app_id: Option<&str>,
        ) -> Result<(), RobotError> {
            self.inner.open_url(device_id, url, app_id).await
        }
        async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
            self.inner.type_text(device_id, text).await
        }
        async fn swipe(
            &self,
            device_id: &str,
            start: Point,
            end: Point,
            duration_ms: Option<u64>,
        ) -> Result<(), RobotError> {
            self.inner.swipe(device_id, start, end, duration_ms).await
        }
        async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
            self.inner.install_app(device_id, apk_path).await
        }
        async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
            self.inner.uninstall_app(device_id, app_id).await
        }
        async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
            let mut elements = self.inner.list_elements(device_id).await?;
            elements[0].text = None;
            Ok(elements)
        }
        async fn list_flutter_elements(
            &self,
            device_id: &str,
        ) -> Result<Vec<UiElement>, RobotError> {
            self.inner.list_flutter_elements(device_id).await
        }
    }

    #[tokio::test]
    async fn live_control_rejects_other_platforms_and_forwards_android_input() {
        let (admin, actions) = state(false);
        let headers = authorized();

        let rejected = android_live(
            Path("browser".into()),
            Query(LiveQuery::default()),
            State(admin.clone()),
            headers.clone(),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

        let offline = android_input(
            Path("offline".into()),
            State(admin.clone()),
            headers.clone(),
            Json(LiveInput {
                action: "tap".into(),
                x: Some(1.0),
                y: Some(2.0),
                end_x: None,
                end_y: None,
                text: None,
                duration_ms: None,
                delta_x: None,
                delta_y: None,
            }),
        )
        .await;
        assert_eq!(offline.status(), StatusCode::BAD_REQUEST);

        let tap = android_input(
            Path("phone".into()),
            State(admin.clone()),
            headers.clone(),
            Json(LiveInput {
                action: "tap".into(),
                x: Some(12.4),
                y: Some(40.0),
                end_x: None,
                end_y: None,
                text: None,
                duration_ms: None,
                delta_x: None,
                delta_y: None,
            }),
        )
        .await;
        assert_eq!(tap.status(), StatusCode::NO_CONTENT);

        let typed = android_input(
            Path("phone".into()),
            State(admin.clone()),
            headers.clone(),
            Json(LiveInput {
                action: "type".into(),
                x: None,
                y: None,
                end_x: None,
                end_y: None,
                text: Some("  hello  ".into()),
                duration_ms: None,
                delta_x: None,
                delta_y: None,
            }),
        )
        .await;
        assert_eq!(typed.status(), StatusCode::NO_CONTENT);

        let missing = android_input(
            Path("phone".into()),
            State(admin),
            headers,
            Json(LiveInput {
                action: "type".into(),
                x: None,
                y: None,
                end_x: None,
                end_y: None,
                text: Some("   ".into()),
                duration_ms: None,
                delta_x: None,
                delta_y: None,
            }),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);

        let recorded = actions.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![
                "tap phone 12.4,40".to_owned(),
                "type phone hello".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn live_endpoints_require_the_admin_token() {
        let (admin, _) = state(false);
        let response = android_live(
            Path("phone".into()),
            Query(LiveQuery::default()),
            State(admin),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

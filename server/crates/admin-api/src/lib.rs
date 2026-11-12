//! Local administration endpoints consumed by the optional Flutter companion.
//!
//! This crate exposes device state and an in-memory activity stream. It has no
//! dependency on MCP protocol dispatch or any concrete platform driver.

use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use robot_core::Robot;
use serde::{Deserialize, Serialize};

mod overlay;

pub use overlay::{AnnotatedScreenshot, with_element_labels};
use tokio::sync::{Mutex, broadcast};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminEvent {
    pub sequence: u64,
    pub kind: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct ActivityLog {
    events: Arc<Mutex<VecDeque<AdminEvent>>>,
    sequence: Arc<AtomicU64>,
    sender: broadcast::Sender<AdminEvent>,
    capacity: usize,
}

impl ActivityLog {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self {
            events: Arc::default(),
            sequence: Arc::default(),
            sender,
            capacity: capacity.max(1),
        }
    }

    pub async fn record(&self, kind: impl Into<String>, detail: impl Into<String>) {
        let event = AdminEvent {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            kind: kind.into(),
            detail: detail.into(),
        };
        let mut events = self.events.lock().await;
        if events.len() == self.capacity {
            events.pop_front();
        }
        events.push_back(event.clone());
        let _ = self.sender.send(event);
    }

    async fn snapshot(&self) -> Vec<AdminEvent> {
        self.events.lock().await.iter().cloned().collect()
    }

    fn subscribe(&self) -> broadcast::Receiver<AdminEvent> {
        self.sender.subscribe()
    }
}

#[derive(Clone)]
pub struct AdminState {
    robot: Arc<dyn Robot>,
    activity_log: ActivityLog,
    bearer_token: Option<Arc<str>>,
}

impl AdminState {
    pub fn new(
        robot: Arc<dyn Robot>,
        activity_log: ActivityLog,
        bearer_token: Option<String>,
    ) -> Self {
        Self {
            robot,
            activity_log,
            bearer_token: bearer_token.map(Arc::from),
        }
    }
}

/// Build unauthenticated route definitions. Each handler validates the bearer
/// token in [`AdminState`] before exposing local device information.
pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/admin/devices", get(list_devices))
        .route("/admin/devices/{device_id}/screenshot", get(screenshot))
        .route("/admin/devices/{device_id}/elements", get(elements))
        .route("/admin/config", get(config))
        .route("/admin/logs", get(logs))
        .route("/admin/logs/stream", get(stream_logs))
        .with_state(state)
}

async fn list_devices(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.robot.list_devices().await {
        Ok(devices) => Json(DevicesResponse { devices }).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Default, Deserialize)]
struct ScreenshotQuery {
    annotate: Option<String>,
}

async fn screenshot(
    Path(device_id): Path<String>,
    Query(query): Query<ScreenshotQuery>,
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(annotate) = query.annotate.as_deref() {
        if annotate != "elements" {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "annotate must be elements".into(),
                }),
            )
                .into_response();
        }
    }
    state
        .activity_log
        .record("admin.screenshot", device_id.clone())
        .await;
    match state.robot.take_screenshot(&device_id).await {
        Ok(png) => {
            let annotated = if query.annotate.as_deref() == Some("elements") {
                match with_element_labels(state.robot.as_ref(), &device_id, png, None).await {
                    Ok(shot) => shot,
                    Err(error) => {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(ErrorResponse {
                                error: error.to_string(),
                            }),
                        )
                            .into_response();
                    }
                }
            } else {
                AnnotatedScreenshot {
                    bytes: png,
                    annotated: false,
                }
            };
            (
                [
                    (header::CONTENT_TYPE, "image/png"),
                    (
                        header::HeaderName::from_static("x-device-bot-annotated"),
                        if annotated.annotated { "true" } else { "false" },
                    ),
                ],
                annotated.bytes,
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElementsQuery {
    filter: Option<String>,
    max_results: Option<usize>,
}

async fn elements(
    Path(device_id): Path<String>,
    Query(query): Query<ElementsQuery>,
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state
        .activity_log
        .record("admin.elements", device_id.clone())
        .await;
    match state
        .robot
        .list_elements_filtered(&device_id, query.filter.as_deref(), query.max_results)
        .await
    {
        Ok(elements) => Json(ElementsResponse { elements }).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn config(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(ConfigResponse {
        auth_enabled: state.bearer_token.is_some(),
        log_capacity: state.activity_log.capacity,
    })
    .into_response()
}

async fn logs(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(LogResponse {
        events: state.activity_log.snapshot().await,
    })
    .into_response()
}

async fn stream_logs(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let stream = BroadcastStream::new(state.activity_log.subscribe()).filter_map(|event| {
        event.ok().map(|event| {
            Ok::<_, Infallible>(
                Event::default()
                    .event("activity")
                    .json_data(event)
                    .expect("serializable event"),
            )
        })
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

fn is_authorized(headers: &HeaderMap, expected_token: Option<&str>) -> bool {
    let Some(expected_token) = expected_token else {
        return true;
    };
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some(expected_token)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicesResponse {
    devices: Vec<robot_core::RobotDevice>,
}

#[derive(Serialize)]
struct ElementsResponse {
    elements: Vec<robot_core::UiElement>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigResponse {
    auth_enabled: bool,
    log_capacity: usize,
}

#[derive(Serialize)]
struct LogResponse {
    events: Vec<AdminEvent>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use robot_core::{Bounds, Point, RobotDevice, RobotError, UiElement};

    struct ScreenshotRobot;

    #[async_trait]
    impl Robot for ScreenshotRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(Vec::new())
        }

        async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
            if device_id == "test-device" {
                Ok(vec![0x89, 0x50, 0x4e, 0x47])
            } else {
                Err(RobotError::DeviceNotFound(device_id.to_owned()))
            }
        }

        async fn tap(&self, _device_id: &str, _point: Point) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn launch_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn open_url(
            &self,
            _device_id: &str,
            _url: &str,
            _app_id: Option<&str>,
        ) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn type_text(&self, _device_id: &str, _text: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn swipe(
            &self,
            _device_id: &str,
            _start: Point,
            _end: Point,
            _duration_ms: Option<u64>,
        ) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn install_app(&self, _device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn uninstall_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
            Err(RobotError::Unsupported("not used in this test".to_owned()))
        }

        async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
            if device_id != "test-device" {
                return Err(RobotError::DeviceNotFound(device_id.to_owned()));
            }
            Ok(vec![UiElement {
                class_name: "Button".to_owned(),
                resource_id: Some("save-button".to_owned()),
                text: Some("Save".to_owned()),
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
            }])
        }
    }

    #[tokio::test]
    async fn retains_a_bounded_event_history_in_order() {
        let log = ActivityLog::new(2);
        log.record("one", "first").await;
        log.record("two", "second").await;
        log.record("three", "third").await;
        let events = log.snapshot().await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 2);
        assert_eq!(events[1].kind, "three");
    }

    #[tokio::test]
    async fn screenshot_endpoint_returns_png_from_the_robot_boundary() {
        let state = AdminState::new(
            Arc::new(ScreenshotRobot),
            ActivityLog::new(2),
            Some("test-token".to_owned()),
        );
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());

        let response = screenshot(
            Path("test-device".to_owned()),
            Query(ScreenshotQuery { annotate: None }),
            State(state),
            headers,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    }

    #[tokio::test]
    async fn elements_endpoint_returns_accessibility_nodes_from_the_robot_boundary() {
        let state = AdminState::new(
            Arc::new(ScreenshotRobot),
            ActivityLog::new(2),
            Some("test-token".to_owned()),
        );
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());

        let response = elements(
            Path("test-device".to_owned()),
            Query(ElementsQuery {
                filter: None,
                max_results: None,
            }),
            State(state),
            headers,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }
}

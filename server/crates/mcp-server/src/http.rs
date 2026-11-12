//! Streamable HTTP transport with optional bearer authentication.
//!
//! HTTP session state is kept here, leaving the protocol dispatcher and tool
//! handlers unaware of transport details.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use device_admin_api::{ActivityLog, AdminState};
use device_mcp_protocol::{JsonRpcRequest, RpcError};
use robot_core::Robot;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{Server, response};

const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Clone)]
struct HttpState {
    robot: Arc<dyn Robot>,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Server>>>>>,
    bearer_token: Option<Arc<str>>,
    activity_log: ActivityLog,
}

/// Build the Streamable HTTP endpoint at `POST /mcp`.
///
/// Clients receive JSON by default. When `Accept: text/event-stream` is sent,
/// the same response is emitted as a single SSE `message` event.
pub fn router(robot: Arc<dyn Robot>, bearer_token: Option<String>) -> Router {
    lan_router(robot, bearer_token, None, None)
}

/// MCP, admin, optional QC web (`/qc`), and optional reports (`/reports`) on one origin.
pub fn lan_router(
    robot: Arc<dyn Robot>,
    bearer_token: Option<String>,
    qc_root: Option<PathBuf>,
    reports: Option<robot_report::Store>,
) -> Router {
    let activity_log = ActivityLog::new(200);
    let state = HttpState {
        robot: Arc::clone(&robot),
        sessions: Arc::default(),
        bearer_token: bearer_token.clone().map(Arc::from),
        activity_log: activity_log.clone(),
    };
    let mut app = Router::new()
        .route("/mcp", post(handle_post).delete(handle_delete))
        .route("/health", get(handle_health))
        .route(
            "/health/live",
            get(|| async {
                axum::Json(json!({
                    "service": "device-bot",
                    "status": "ok",
                    "version": env!("CARGO_PKG_VERSION"),
                }))
            }),
        )
        .route("/reboot", post(handle_reboot))
        .with_state(state)
        .merge(device_admin_api::router(AdminState::new(
            robot,
            activity_log,
            bearer_token,
        )));
    if let Some(store) = reports {
        app = app.nest("/reports", robot_report::router(store));
    }
    if let Some(root) = qc_root {
        app = app.nest("/qc", crate::lan::qc_router(root));
    }
    app.fallback(lan_options_fallback)
        .layer(middleware::from_fn(crate::lan::cors))
}

async fn lan_options_fallback(request: Request) -> Response {
    if request.method() == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthTarget {
    device_id: Option<String>,
}

async fn handle_health(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(target): Query<HealthTarget>,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let devices = match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        state.robot.list_devices(),
    )
    .await
    {
        Ok(Ok(devices)) => devices,
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(
                    json!({"healthy":false,"detail":"Device discovery failed or timed out"}),
                ),
            )
                .into_response();
        }
    };
    if target
        .device_id
        .as_ref()
        .is_some_and(|id| !devices.iter().any(|d| &d.id == id))
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"healthy":false,"detail":"Device not found"})),
        )
            .into_response();
    }
    let mut checks = Vec::new();
    let mut probes = tokio::task::JoinSet::new();
    for device in devices
        .into_iter()
        .filter(|d| target.device_id.as_ref().is_none_or(|id| &d.id == id))
    {
        let robot = state.robot.clone();
        probes.spawn(async move {
            // Real-iOS health may open an idle Wi-Fi tunnel after device discovery.
            let status = tokio::time::timeout(std::time::Duration::from_secs(10), robot.check_device_health(&device.id)).await;
            let mut value = match status {
                Ok(Ok(status)) => json!(status),
                Ok(Err(error)) => json!({"ready":false,"state":"unverified","detail":error.to_string()}),
                Err(_) => json!({"ready":false,"state":"timeout","detail":"Read-only helper check timed out"}),
            };
            value["device"] = json!(device);
            value
        });
    }
    while let Some(result) = probes.join_next().await {
        match result {
            Ok(value) => checks.push(value),
            Err(_) => checks.push(json!({"ready":false,"state":"check_failed"})),
        }
    }
    checks.sort_by(|a, b| a["device"]["id"].as_str().cmp(&b["device"]["id"].as_str()));
    let ready = !checks.is_empty() && checks.iter().all(|v| v["ready"] == true);
    let code = if target.device_id.is_some() && !ready {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (code, axum::Json(json!({"server":{"healthy":true,"name":"device-bot","version":env!("CARGO_PKG_VERSION"),"protocolVersion":device_mcp_protocol::PROTOCOL_VERSION},"ready":ready,"devices":checks}))).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RebootTarget {
    device_id: String,
    app_id: Option<String>,
    #[serde(default = "default_reinstall")]
    reinstall: bool,
}
fn default_reinstall() -> bool {
    true
}

async fn handle_reboot(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let target =
        match serde_json::from_slice::<RebootTarget>(&body) {
            Ok(target)
                if !target.device_id.trim().is_empty()
                    && target
                        .app_id
                        .as_ref()
                        .is_none_or(|id| !id.trim().is_empty()) =>
            {
                target
            }
            _ => return (
                StatusCode::BAD_REQUEST,
                axum::Json(
                    json!({"detail":"Expected deviceId and optional non-empty appId/reinstall"}),
                ),
            )
                .into_response(),
        };
    // Reinstall only the helper. Never reset/reboot the device or uninstall its target app.
    match tokio::time::timeout(std::time::Duration::from_secs(300), async {
        if target.reinstall { state.robot.reboot_helper(&target.device_id, target.app_id.as_deref()).await }
        else { state.robot.prepare_helper(&target.device_id, target.app_id.as_deref()).await }
    }).await {
        Ok(Ok(status)) => (StatusCode::OK, axum::Json(json!({"deviceId":target.device_id,"serverVersion":env!("CARGO_PKG_VERSION"),"runner":status}))).into_response(),
        Ok(Err(error)) => (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"deviceId":target.device_id,"detail":error.to_string()}))).into_response(),
        Err(_) => (StatusCode::GATEWAY_TIMEOUT, axum::Json(json!({"deviceId":target.device_id,"detail":"Helper preparation timed out"}))).into_response(),
    }
}

/// Bind and serve Streamable HTTP.
pub async fn serve(
    address: SocketAddr,
    robot: Arc<dyn Robot>,
    bearer_token: Option<String>,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(robot, bearer_token)).await
}

async fn handle_post(State(state): State<HttpState>, headers: HeaderMap, body: Bytes) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let raw_request = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return json_response(
                response::error_for_id(Value::Null, RpcError::parse_error()),
                None,
                &headers,
            );
        }
    };
    let request = match serde_json::from_value::<JsonRpcRequest>(raw_request) {
        Ok(request) => request,
        Err(_) => {
            return json_response(
                response::error_for_id(Value::Null, RpcError::invalid_request()),
                None,
                &headers,
            );
        }
    };

    state
        .activity_log
        .record("mcp.request", request.method.clone())
        .await;

    if request.method == "initialize" {
        initialize_session(state, headers, request).await
    } else {
        dispatch_session(state, headers, request).await
    }
}

async fn initialize_session(
    state: HttpState,
    headers: HeaderMap,
    request: JsonRpcRequest,
) -> Response {
    let mut server = Server::new(
        state
            .robot
            .fork_session()
            .unwrap_or_else(|| Arc::clone(&state.robot)),
    );
    let result = server.handle(request).await;
    let initialized = result
        .as_ref()
        .is_some_and(|value| value.get("result").is_some());
    if !initialized {
        return response_or_accepted(result, None, &headers);
    }

    let session_id = Uuid::new_v4().to_string();
    state
        .sessions
        .lock()
        .await
        .insert(session_id.clone(), Arc::new(Mutex::new(server)));
    response_or_accepted(result, Some(&session_id), &headers)
}

async fn dispatch_session(
    state: HttpState,
    headers: HeaderMap,
    request: JsonRpcRequest,
) -> Response {
    let Some(session_id) = headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(server) = state.sessions.lock().await.get(session_id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let result = server.lock().await.handle(request).await;
    response_or_accepted(result, None, &headers)
}

async fn handle_delete(State(state): State<HttpState>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(session_id) = headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let removed = state.sessions.lock().await.remove(session_id);
    if let Some(server) = removed {
        match server.lock().await.shutdown().await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
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

fn response_or_accepted(
    response: Option<Value>,
    session_id: Option<&str>,
    headers: &HeaderMap,
) -> Response {
    match response {
        Some(response) => json_response(response, session_id, headers),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn json_response(value: Value, session_id: Option<&str>, headers: &HeaderMap) -> Response {
    let mut response = if accepts_sse(headers) {
        let event = format!("event: message\ndata: {value}\n\n");
        (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            )],
            event,
        )
            .into_response()
    } else {
        axum::Json(value).into_response()
    };
    if let Some(session_id) = session_id {
        response.headers_mut().insert(
            SESSION_HEADER,
            HeaderValue::from_str(session_id).expect("UUID is a valid header value"),
        );
    }
    response
}

fn accepts_sse(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|item| item.trim() == "text/event-stream")
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use robot_core::{Point, Robot, RobotDevice, RobotError, UiElement};
    use tower::ServiceExt;

    use super::*;

    #[derive(Default)]
    struct NoopRobot {
        preparations: std::sync::Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl Robot for NoopRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(vec![RobotDevice {
                id: "sim".into(),
                name: "Simulator".into(),
                platform: robot_core::Platform::Ios,
                state: robot_core::DeviceState::Available,
            }])
        }
        async fn ui_runner_status(
            &self,
            _: &str,
        ) -> Result<robot_core::UiRunnerStatus, RobotError> {
            Ok(robot_core::UiRunnerStatus {
                ready: false,
                state: "disconnected".into(),
                detail: "No live helper".into(),
                cache_ready: true,
                expected_version: Some("v2".into()),
                ..Default::default()
            })
        }
        async fn prepare_ui_runner(
            &self,
            id: &str,
            app: Option<&str>,
            reinstall: bool,
        ) -> Result<robot_core::UiRunnerStatus, RobotError> {
            self.preparations
                .lock()
                .unwrap()
                .push(json!({"deviceId":id,"appId":app,"reinstall":reinstall}));
            Ok(robot_core::UiRunnerStatus {
                ready: true,
                state: "ready".into(),
                detail: "prepared".into(),
                ..Default::default()
            })
        }
        async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
            Err(RobotError::Unsupported("unused".into()))
        }
        async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
            Ok(())
        }
        async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
            Ok(())
        }
        async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn swipe(
            &self,
            _: &str,
            _: Point,
            _: Point,
            _: Option<u64>,
        ) -> Result<(), RobotError> {
            Ok(())
        }
        async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
            Ok(())
        }
        async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
            Ok(vec![])
        }
    }

    fn initialize_body() -> &'static str {
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}"#
    }

    #[tokio::test]
    async fn requires_bearer_auth_and_preserves_lifecycle_in_one_session() {
        let app = router(Arc::new(NoopRobot::default()), Some("secret".into()));
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .body(Body::from(initialize_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let initialized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::from(initialize_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initialized.status(), StatusCode::OK);
        let session = initialized.headers()[SESSION_HEADER]
            .to_str()
            .unwrap()
            .to_owned();

        let before_ready = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(SESSION_HEADER, session)
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(before_ready.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn returns_an_sse_message_when_the_client_requests_it() {
        let response = router(Arc::new(NoopRobot::default()), None)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header(header::ACCEPT, "text/event-stream")
                    .body(Body::from(initialize_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
    }
    #[tokio::test]
    async fn lan_preflight_allows_private_origins_and_exposes_the_session_header() {
        let response = router(Arc::new(NoopRobot::default()), Some("secret".into()))
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/mcp")
                    .header(header::ORIGIN, "http://192.168.1.20:3337")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,mcp-session-id",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://192.168.1.20:3337"
        );
        let allowed = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap();
        assert!(allowed.to_ascii_lowercase().contains("authorization"));
        assert!(allowed.contains("mcp-session-id"));
        assert!(
            response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
                .to_str()
                .unwrap()
                .contains("mcp-session-id")
        );
        let denied = router(Arc::new(NoopRobot::default()), Some("secret".into()))
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/mcp")
                    .header(header::ORIGIN, "http://example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            denied
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[tokio::test]
    async fn liveness_is_available_without_authentication_or_device_checks() {
        let response = router(Arc::new(NoopRobot::default()), Some("secret".into()))
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["service"], "device-bot");
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn health_is_read_only_and_reboot_is_authenticated_and_device_scoped() {
        let robot = Arc::new(NoopRobot::default());
        let app = router(robot.clone(), Some("test-secret".into()));
        for (method, uri, body) in [
            ("GET", "/health?deviceId=sim", ""),
            ("POST", "/reboot", r#"{"deviceId":"sim"}"#),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        assert!(robot.preparations.lock().unwrap().is_empty());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health?deviceId=sim")
                    .header(header::AUTHORIZATION, "Bearer test-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        let status: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status["server"]["healthy"], true);
        assert_eq!(status["devices"][0]["ready"], false);
        assert_eq!(status["devices"][0]["cacheReady"], true);
        assert!(robot.preparations.lock().unwrap().is_empty());
        for (body, expected) in [
            (r#"{"deviceId":"sim", "appId":"app"}"#, true),
            (r#"{"deviceId":"sim", "reinstall":false}"#, false),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/reboot")
                        .header(header::AUTHORIZATION, "Bearer test-secret")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                robot.preparations.lock().unwrap().last().unwrap()["reinstall"],
                expected
            );
        }
        for body in [
            r#"{"deviceId":""}"#,
            r#"{"deviceId":"sim","appId":""}"#,
            r#"{"deviceId":"sim", "wipe":true}"#,
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/reboot")
                        .header(header::AUTHORIZATION, "Bearer test-secret")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(robot.preparations.lock().unwrap().len(), 2);
    }
}

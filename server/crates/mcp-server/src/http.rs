//! Streamable HTTP transport with optional bearer authentication.
//!
//! HTTP session state is kept here, leaving the protocol dispatcher and tool
//! handlers unaware of transport details.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use mobile_admin_api::{ActivityLog, AdminState};
use mobile_mcp_protocol::{JsonRpcRequest, RpcError};
use mobile_robot_core::Robot;
use serde_json::Value;
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
    let activity_log = ActivityLog::new(200);
    let state = HttpState {
        robot: Arc::clone(&robot),
        sessions: Arc::default(),
        bearer_token: bearer_token.clone().map(Arc::from),
        activity_log: activity_log.clone(),
    };
    Router::new()
        .route("/mcp", post(handle_post).delete(handle_delete))
        .with_state(state)
        .merge(mobile_admin_api::router(AdminState::new(
            robot,
            activity_log,
            bearer_token,
        )))
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
    let mut server = Server::new(Arc::clone(&state.robot));
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
    if state.sessions.lock().await.remove(session_id).is_some() {
        StatusCode::NO_CONTENT.into_response()
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
    use mobile_robot_core::{Point, Robot, RobotDevice, RobotError, UiElement};
    use tower::ServiceExt;

    use super::*;

    struct NoopRobot;

    #[async_trait]
    impl Robot for NoopRobot {
        async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
            Ok(vec![])
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
        let app = router(Arc::new(NoopRobot), Some("secret".into()));
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
        let response = router(Arc::new(NoopRobot), None)
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
}

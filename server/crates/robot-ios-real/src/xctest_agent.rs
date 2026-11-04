use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

use robot_core::RobotError;

const TOKEN_HEADER: &str = "x-mobilemcp-token";

#[derive(Clone)]
pub struct XCTestAgentBridge {
    state: Arc<StateData>,
}

struct StateData {
    token: Option<String>,
    commands: Mutex<VecDeque<AgentCommand>>,
    waiters: Mutex<HashMap<String, oneshot::Sender<AgentResult>>>,
    last_authenticated_request: Mutex<Option<std::time::Instant>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommand {
    pub id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentResult {
    id: String,
    ok: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(flatten)]
    payload: serde_json::Map<String, Value>,
}

impl XCTestAgentBridge {
    pub fn new(token: Option<String>) -> Self {
        Self {
            state: Arc::new(StateData {
                token: token.filter(|value| !value.is_empty()),
                commands: Mutex::new(VecDeque::new()),
                waiters: Mutex::new(HashMap::new()),
                last_authenticated_request: Mutex::new(None),
            }),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/next", get(next))
            .route("/result", post(result))
            .with_state(Arc::clone(&self.state))
    }

    pub async fn dispatch(
        &self,
        method: impl Into<String>,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RobotError> {
        let id = Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.state.waiters.lock().await.insert(id.clone(), sender);
        self.state.commands.lock().await.push_back(AgentCommand {
            id: id.clone(),
            method: method.into(),
            params,
        });

        let result = tokio::time::timeout(timeout, receiver).await;
        if result.is_err() {
            self.state.waiters.lock().await.remove(&id);
            self.state
                .commands
                .lock()
                .await
                .retain(|command| command.id != id);
            return Err(RobotError::CommandFailed(
                self.connection_diagnostic().await,
            ));
        }
        let result = result.unwrap().map_err(|_| {
            RobotError::CommandFailed(
                "iOS XCTest agent disconnected before returning a result".into(),
            )
        })?;
        if result.ok {
            return Ok(Value::Object(result.payload));
        }
        Err(RobotError::CommandFailed(
            result
                .message
                .unwrap_or_else(|| "iOS XCTest action failed".into()),
        ))
    }

    pub async fn connection_diagnostic(&self) -> String {
        match *self.state.last_authenticated_request.lock().await {
            Some(last_seen) => format!(
                "iOS XCTest agent last contacted the bridge {}s ago but did not respond; inspect its XCTest log",
                last_seen.elapsed().as_secs()
            ),
            None => "no authenticated iOS XCTest agent has contacted the bridge; provision the runner and configure a reachable DEVICE_BOT_IOS_AGENT_HOSTS value".into(),
        }
    }
}

async fn health(State(state): State<Arc<StateData>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let bridge = XCTestAgentBridge { state };
    match bridge.dispatch("health", serde_json::json!({}), Duration::from_secs(2)).await {
        // dispatch already validates AgentResult.ok; it is not part of the payload.
        Ok(_) =>
            Json(serde_json::json!({"healthy":true})).into_response(),
        _ => (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"healthy":false,"detail":"XCTest helper did not confirm its heartbeat"})))
            .into_response(),
    }
}

fn authorized(headers: &HeaderMap, state: &StateData) -> bool {
    let Some(token) = state.token.as_deref() else {
        return true;
    };
    headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == token)
}

async fn next(State(state): State<Arc<StateData>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    *state.last_authenticated_request.lock().await = Some(std::time::Instant::now());
    match state.commands.lock().await.pop_front() {
        Some(command) => Json(command).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn result(
    State(state): State<Arc<StateData>>,
    headers: HeaderMap,
    Json(result): Json<AgentResult>,
) -> Response {
    if !authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    *state.last_authenticated_request.lock().await = Some(std::time::Instant::now());
    if let Some(waiter) = state.waiters.lock().await.remove(&result.id) {
        let _ = waiter.send(result);
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_endpoint_requires_authentication_and_a_live_round_trip() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let bridge = XCTestAgentBridge::new(Some("test-token".into()));
        let response = bridge
            .router()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(bridge.state.commands.lock().await.is_empty());
        let pending = tokio::spawn(
            bridge.router().oneshot(
                Request::builder()
                    .uri("/health")
                    .header(TOKEN_HEADER, "test-token")
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        let command = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(c) = bridge.state.commands.lock().await.pop_front() {
                    break c;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(command.method, "health");
        // Exercise the same JSON decoding as a real Swift helper response.
        let result = bridge
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/result")
                    .header(TOKEN_HEADER, "test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": command.id,
                            "ok": true,
                            "bundleId": "com.example.app"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::NO_CONTENT);
        let response = pending.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["healthy"],
            true
        );
    }

    #[tokio::test]
    async fn health_endpoint_reports_nonresponsive_helper_and_clears_probe() {
        use tower::ServiceExt;
        let bridge = XCTestAgentBridge::new(None);
        let response = bridge
            .router()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(bridge.state.commands.lock().await.is_empty());
        assert!(bridge.state.waiters.lock().await.is_empty());
    }

    #[tokio::test]
    async fn dispatches_a_command_and_matches_its_result() {
        let bridge = XCTestAgentBridge::new(Some("secret".into()));
        let waiting = {
            let bridge = bridge.clone();
            tokio::spawn(async move {
                bridge
                    .dispatch("health", serde_json::json!({}), Duration::from_secs(1))
                    .await
                    .unwrap()
            })
        };
        tokio::task::yield_now().await;
        let command = bridge.state.commands.lock().await.pop_front().unwrap();
        let waiter = bridge
            .state
            .waiters
            .lock()
            .await
            .remove(&command.id)
            .unwrap();
        waiter
            .send(AgentResult {
                id: command.id,
                ok: true,
                message: None,
                payload: serde_json::Map::from_iter([("ready".into(), Value::Bool(true))]),
            })
            .unwrap();
        assert_eq!(waiting.await.unwrap(), serde_json::json!({ "ready": true }));
    }

    #[tokio::test]
    async fn timeout_removes_the_stale_command_from_the_poll_queue() {
        let bridge = XCTestAgentBridge::new(None);

        let error = bridge
            .dispatch("health", serde_json::json!({}), Duration::from_millis(1))
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("no authenticated iOS XCTest agent")
        );
        assert!(bridge.state.commands.lock().await.is_empty());
        assert!(bridge.state.waiters.lock().await.is_empty());
    }
}

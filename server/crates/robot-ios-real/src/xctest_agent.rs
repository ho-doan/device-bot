use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Query, State},
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
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
        self.dispatch_to(None, method.into(), params, timeout).await
    }

    pub async fn dispatch_for(
        &self,
        device_id: &str,
        method: impl Into<String>,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RobotError> {
        self.dispatch_to(Some(device_id.into()), method.into(), params, timeout)
            .await
    }

    async fn dispatch_to(
        &self,
        device_id: Option<String>,
        method: String,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RobotError> {
        let id = Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.state.waiters.lock().await.insert(id.clone(), sender);
        self.state.commands.lock().await.push_back(AgentCommand {
            id: id.clone(),
            method,
            params,
            device_id,
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

async fn health(
    State(state): State<Arc<StateData>>,
    Query(identity): Query<AgentIdentity>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let bridge = XCTestAgentBridge { state };
    let result = if let Some(device) = identity.device_id.as_deref() {
        bridge
            .dispatch_for(
                device,
                "health",
                serde_json::json!({}),
                Duration::from_secs(2),
            )
            .await
    } else {
        bridge
            .dispatch("health", serde_json::json!({}), Duration::from_secs(2))
            .await
    };
    match result {
        Ok(value) => {
            let matches = identity.device_id.as_ref().is_none_or(|id| value["deviceId"].as_str() == Some(id))
                && identity.expected_version.as_ref().is_none_or(|v| value["agentVersion"].as_str() == Some(v));
            (if matches { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
                Json(serde_json::json!({"healthy":matches,"helper":value}))).into_response()
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"healthy":false,"detail":"XCTest helper did not confirm its heartbeat"}))).into_response(),
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

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentIdentity {
    device_id: Option<String>,
    expected_version: Option<String>,
}

async fn next(
    State(state): State<Arc<StateData>>,
    Query(identity): Query<AgentIdentity>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    *state.last_authenticated_request.lock().await = Some(std::time::Instant::now());
    let mut commands = state.commands.lock().await;
    let index = commands
        .iter()
        .position(|command| command.device_id.is_none() || command.device_id == identity.device_id);
    match index.and_then(|index| commands.remove(index)) {
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
    #[tokio::test]
    async fn helper_on_another_device_cannot_consume_scoped_commands() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let bridge = XCTestAgentBridge::new(None);
        let sender = bridge.clone();
        let task = tokio::spawn(async move {
            sender
                .dispatch_for(
                    "sim-a",
                    "health",
                    serde_json::json!({}),
                    Duration::from_secs(2),
                )
                .await
        });
        tokio::task::yield_now().await;
        for path in ["/next?deviceId=sim-b", "/next"] {
            let response = bridge
                .router()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
        let response = bridge
            .router()
            .oneshot(
                Request::builder()
                    .uri("/next?deviceId=sim-a")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let command: Value = serde_json::from_slice(&bytes).unwrap();
        let response = bridge.router().oneshot(Request::builder().method("POST").uri("/result").header("content-type","application/json").body(Body::from(serde_json::json!({"id":command["id"],"ok":true,"deviceId":"sim-a","agentVersion":"v2"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(task.await.unwrap().unwrap()["deviceId"], "sim-a");
    }
}

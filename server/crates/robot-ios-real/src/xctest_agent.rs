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

use mobile_robot_core::RobotError;

const TOKEN_HEADER: &str = "x-mobilemcp-token";

#[derive(Clone)]
pub struct XCTestAgentBridge {
    state: Arc<StateData>,
}

struct StateData {
    token: Option<String>,
    commands: Mutex<VecDeque<AgentCommand>>,
    waiters: Mutex<HashMap<String, oneshot::Sender<AgentResult>>>,
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
            }),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
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
            return Err(RobotError::CommandFailed(
                "iOS XCTest agent did not respond; build and start server/crates/robot-ios-real/xctest-runner on the paired device".into(),
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
}

use std::sync::Arc;

use async_trait::async_trait;
use mobile_mcp_protocol::{JsonRpcRequest, PROTOCOL_VERSION};
use mobile_mcp_server::Server;
use mobile_robot_core::{Point, Robot, RobotDevice, RobotError};
use serde_json::{Value, json};

struct NoopRobot;

#[async_trait]
impl Robot for NoopRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![])
    }

    async fn take_screenshot(&self, _device_id: &str) -> Result<Vec<u8>, RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn tap(&self, _device_id: &str, _point: Point) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn launch_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn open_url(
        &self,
        _device_id: &str,
        _url: &str,
        _app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn type_text(&self, _device_id: &str, _text: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn swipe(
        &self,
        _device_id: &str,
        _start: Point,
        _end: Point,
        _duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn install_app(&self, _device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn uninstall_app(&self, _device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }

    async fn list_elements(
        &self,
        _device_id: &str,
    ) -> Result<Vec<mobile_robot_core::UiElement>, RobotError> {
        Err(RobotError::Unsupported(
            "not used by lifecycle tests".into(),
        ))
    }
}

fn request(id: u64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(id)),
        method: method.into(),
        params: Some(params),
    }
}

async fn initialize(server: &mut Server) {
    let response = server
        .handle(request(
            1,
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.1.0" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
}

#[tokio::test]
async fn initialization_and_tools_list_follow_the_lifecycle() {
    let mut server = Server::new(Arc::new(NoopRobot));

    let before_ready = server
        .handle(request(1, "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(before_ready["error"]["code"], -32600);

    initialize(&mut server).await;
    assert_eq!(
        server
            .handle(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "notifications/initialized".into(),
                params: None,
            })
            .await,
        None
    );

    let tools = server
        .handle(request(2, "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 13);
}

#[tokio::test]
async fn unknown_tools_return_an_invalid_params_error() {
    let mut server = Server::new(Arc::new(NoopRobot));
    initialize(&mut server).await;
    server
        .handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        })
        .await;

    let result = server
        .handle(request(2, "tools/call", json!({ "name": "mobile_tap" })))
        .await
        .unwrap();
    assert_eq!(result["error"]["code"], -32602);
    assert_eq!(result["error"]["message"], "Unknown tool: mobile_tap");
}

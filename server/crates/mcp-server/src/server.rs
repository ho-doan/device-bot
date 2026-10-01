use std::sync::Arc;

use device_mcp_protocol::{
    CallToolParams, Implementation, InitializeParams, InitializeResult, JsonRpcRequest,
    ListToolsResult, PROTOCOL_VERSION, RpcError, ServerCapabilities, ToolsCapability,
};
use robot_core::Robot;
use serde_json::{Value, json};

use crate::{lifecycle::Lifecycle, response, tools::ToolRegistry};

/// Stateful MCP dispatcher for one transport connection.
pub struct Server {
    lifecycle: Lifecycle,
    tools: ToolRegistry,
}

impl Server {
    pub fn new(robot: Arc<dyn Robot>) -> Self {
        Self {
            lifecycle: Lifecycle::default(),
            tools: ToolRegistry::new(robot),
        }
    }

    /// Dispatch one syntactically valid JSON-RPC message.
    ///
    /// `None` means a JSON-RPC notification was handled and must not receive a
    /// response. Returned values are complete JSON-RPC response objects.
    pub async fn handle(&mut self, request: JsonRpcRequest) -> Option<Value> {
        if !request.has_valid_version() {
            return response::error(&request, RpcError::invalid_request());
        }

        match request.method.as_str() {
            "initialize" => self.initialize(request),
            "notifications/initialized" => self.initialized(),
            "ping" => response::success(&request, json!({})),
            "tools/list" => self.list_tools(request),
            "tools/call" => self.call_tool(request).await,
            _ => response::error(&request, RpcError::method_not_found(&request.method)),
        }
    }

    fn initialize(&mut self, request: JsonRpcRequest) -> Option<Value> {
        let params = match request.params.clone() {
            Some(params) => match serde_json::from_value::<InitializeParams>(params) {
                Ok(params) => params,
                Err(error) => {
                    return response::error(&request, RpcError::invalid_params(error.to_string()));
                }
            },
            None => {
                return response::error(
                    &request,
                    RpcError::invalid_params("Missing initialize params"),
                );
            }
        };

        if !self.lifecycle.begin_initialization() {
            return response::error(
                &request,
                RpcError::invalid_request().with_data(json!({
                    "reason": "initialize may only be called once per connection"
                })),
            );
        }

        // The server deliberately has one current protocol version. Per MCP
        // negotiation rules, returning it lets an older client disconnect if
        // it cannot support that version.
        let _requested_version = params.protocol_version;
        let _client = params.client_info;
        response::success(
            &request,
            InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {},
                },
                server_info: Implementation {
                    name: "device-bot".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                },
            },
        )
    }

    fn initialized(&mut self) -> Option<Value> {
        self.lifecycle.mark_ready();
        None
    }

    fn list_tools(&self, request: JsonRpcRequest) -> Option<Value> {
        if let Err(error) = self.require_ready(&request) {
            return Some(error);
        }
        response::success(
            &request,
            ListToolsResult {
                tools: self.tools.definitions(),
            },
        )
    }

    async fn call_tool(&self, request: JsonRpcRequest) -> Option<Value> {
        if let Err(error) = self.require_ready(&request) {
            return Some(error);
        }

        let params = match request.params.clone() {
            Some(params) => match serde_json::from_value::<CallToolParams>(params) {
                Ok(params) => params,
                Err(error) => {
                    return response::error(&request, RpcError::invalid_params(error.to_string()));
                }
            },
            None => {
                return response::error(
                    &request,
                    RpcError::invalid_params("Missing tools/call params"),
                );
            }
        };

        match self.tools.call(params).await {
            Ok(result) => response::success(&request, result),
            Err(error) => response::error(&request, error),
        }
    }

    fn require_ready(&self, request: &JsonRpcRequest) -> Result<(), Value> {
        if self.lifecycle.is_ready() {
            Ok(())
        } else {
            Err(response::error_value(
                request,
                RpcError::invalid_request().with_data(json!({
                    "reason": "Client must send notifications/initialized first"
                })),
            ))
        }
    }
}

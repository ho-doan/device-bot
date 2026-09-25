//! Wire types shared by Mobile MCP transports and servers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Latest MCP version supported by this server.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// A JSON-RPC request or notification received from an MCP client.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    pub fn has_valid_version(&self) -> bool {
        self.jsonrpc == "2.0"
    }
}

/// JSON-RPC success response.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JsonRpcSuccess<T> {
    pub jsonrpc: &'static str,
    pub id: Value,
    pub result: T,
}

impl<T> JsonRpcSuccess<T> {
    pub fn new(id: Value, result: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result,
        }
    }
}

/// JSON-RPC error response.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JsonRpcError {
    pub jsonrpc: &'static str,
    pub id: Value,
    pub error: RpcError,
}

impl JsonRpcError {
    pub fn new(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            error,
        }
    }
}

/// Standard JSON-RPC error object.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, format!("Method not found: {method}"))
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Information supplied by the MCP client during initialization.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Value,
    pub client_info: Implementation,
}

/// Identifies an MCP client or server implementation.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

/// Result returned from `initialize`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: &'static str,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
}

/// Server capability declaration for Phase 0.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
}

/// The server exposes tools but does not mutate the catalogue yet.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolsCapability {}

/// Result returned from `tools/list`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ListToolsResult {
    pub tools: Vec<ToolDefinition>,
}

/// Definition exposed in an MCP tool catalogue.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
}

/// Parameters supplied to `tools/call`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default = "empty_object")]
    pub arguments: Value,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

/// Result of a successful or failed tool execution.
///
/// Execution failures are deliberately represented here rather than as a
/// JSON-RPC error so the MCP client can show the model actionable feedback.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResult {
    pub fn structured(value: Value) -> Self {
        let text = serde_json::to_string(&value).expect("serializable structured tool output");
        Self {
            content: vec![ToolContent::Text { text }],
            structured_content: Some(value),
            is_error: None,
        }
    }

    pub fn image(data: String, mime_type: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Image {
                data,
                mime_type: mime_type.into(),
            }],
            structured_content: None,
            is_error: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            structured_content: None,
            is_error: None,
        }
    }

    pub fn execution_error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: message.into(),
            }],
            structured_content: None,
            is_error: Some(true),
        }
    }
}

/// MCP content types used by the initial Android vertical slice.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserializes_a_json_rpc_request() {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/list"
        }))
        .unwrap();

        assert_eq!(request.id, Some(json!(7)));
        assert!(!request.is_notification());
        assert!(request.has_valid_version());
    }

    #[test]
    fn serializes_empty_tool_catalogue() {
        let result = ListToolsResult { tools: vec![] };
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "tools": [] })
        );
    }

    #[test]
    fn serializes_a_structured_tool_result_with_text_fallback() {
        let result = ToolResult::structured(json!({ "devices": [] }));
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "content": [{ "type": "text", "text": "{\"devices\":[]}" }],
                "structuredContent": { "devices": [] }
            })
        );
    }
}

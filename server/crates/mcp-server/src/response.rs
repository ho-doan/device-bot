use mobile_mcp_protocol::{JsonRpcError, JsonRpcRequest, JsonRpcSuccess, RpcError};
use serde::Serialize;
use serde_json::Value;

pub(crate) fn success<T: Serialize>(request: &JsonRpcRequest, result: T) -> Option<Value> {
    request.id.clone().map(|id| {
        serde_json::to_value(JsonRpcSuccess::new(id, result)).expect("serializable response")
    })
}

pub(crate) fn error(request: &JsonRpcRequest, error: RpcError) -> Option<Value> {
    request.id.as_ref().map(|_| error_value(request, error))
}

pub(crate) fn error_value(request: &JsonRpcRequest, error: RpcError) -> Value {
    error_for_id(request.id.clone().unwrap_or(Value::Null), error)
}

pub(crate) fn error_for_id(id: Value, error: RpcError) -> Value {
    serde_json::to_value(JsonRpcError::new(id, error)).expect("serializable response")
}

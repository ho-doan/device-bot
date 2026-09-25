use mobile_mcp_protocol::{JsonRpcRequest, PROTOCOL_VERSION};
use mobile_mcp_server::Server;
use serde_json::{Value, json};

fn request(id: u64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(id)),
        method: method.into(),
        params: Some(params),
    }
}

fn initialize(server: &mut Server) {
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
        .unwrap();
    assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
}

#[test]
fn initialization_and_tools_list_follow_the_lifecycle() {
    let mut server = Server::default();

    // `tools/list` is not legal before the initialized notification.
    let before_ready = server.handle(request(1, "tools/list", json!({}))).unwrap();
    assert_eq!(before_ready["error"]["code"], -32600);

    initialize(&mut server);
    assert_eq!(
        server.handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        }),
        None
    );

    let tools = server.handle(request(2, "tools/list", json!({}))).unwrap();
    assert_eq!(
        tools,
        json!({ "jsonrpc": "2.0", "id": 2, "result": { "tools": [] } })
    );
}

#[test]
fn unknown_tools_return_an_invalid_params_error() {
    let mut server = Server::default();
    initialize(&mut server);
    server.handle(JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: None,
        method: "notifications/initialized".into(),
        params: None,
    });

    let result = server
        .handle(request(2, "tools/call", json!({ "name": "mobile_tap" })))
        .unwrap();
    assert_eq!(result["error"]["code"], -32602);
    assert_eq!(result["error"]["message"], "Unknown tool: mobile_tap");
}

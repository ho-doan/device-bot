//! Newline-delimited JSON-RPC transport for MCP stdio clients.

use std::io;

use device_mcp_protocol::{JsonRpcRequest, RpcError};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{Server, response};

/// Serve one MCP connection through process stdin/stdout.
pub async fn serve(mut server: Server) -> io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => server.handle(request).await,
            Err(_) => Some(response::error_for_id(
                serde_json::Value::Null,
                RpcError::parse_error(),
            )),
        };

        if let Some(response) = response {
            let encoded = serde_json::to_string(&response).expect("serializable response");
            stdout.write_all(encoded.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use device_mcp_protocol::RpcError;
    use serde_json::{Value, json};

    use crate::response;

    #[test]
    fn parse_errors_use_json_rpc_standard_shape() {
        assert_eq!(
            response::error_for_id(Value::Null, RpcError::parse_error()),
            json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": "Parse error" }
            })
        );
    }
}

//! HTTP transport for running YAML against the same MCP backend used by QC.
use crate::mcp::{Mcp, Result};
use async_trait::async_trait;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, Uri, body::Bytes};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

pub struct HttpMcp {
    uri: Uri,
    session: Option<String>,
    token: Option<String>,
    next_id: u64,
    timeout: Duration,
}

impl HttpMcp {
    pub fn new(url: &str, timeout: Duration) -> Result<Self> {
        let uri: Uri = url.parse().map_err(|_| "invalid MCP HTTP URL")?;
        if uri.scheme_str() != Some("http")
            || !matches!(
                uri.host(),
                Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
            )
        {
            return Err("--mcp-url currently requires a loopback http:// URL".into());
        }
        let token = std::env::var("DEVICE_BOT_AUTH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
                let port = uri.port_u16().unwrap_or(80);
                // Dev writes ~/.device-bot-dev; prod writes ~/.device-bot. The stored URL
                // is checked below, so a file for the other flavor cannot authenticate.
                let path = [".device-bot-dev", ".device-bot"]
                    .into_iter()
                    .find_map(|dir| {
                        let path = PathBuf::from(&home)
                            .join(dir)
                            .join("mcp")
                            .join(format!("connection-{port}.json"));
                        path.is_file().then_some(path)
                    })?;
                let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
                let stored: Uri = value["url"].as_str()?.parse().ok()?;
                if stored.scheme_str() != Some("http")
                    || stored.port_u16().unwrap_or(80) != uri.port_u16().unwrap_or(80)
                    || !matches!(
                        stored.host(),
                        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
                    )
                {
                    return None;
                }
                value["bearerToken"].as_str().map(str::to_owned)
            });
        Ok(Self {
            uri,
            session: None,
            token,
            next_id: 1,
            timeout,
        })
    }

    async fn exchange(&mut self, method: Method, body: Option<Value>) -> Result<Value> {
        tokio::time::timeout(self.timeout, async {
            let authority = self.uri.authority().ok_or("MCP URL has no host")?.as_str();
            let address = if self.uri.port_u16().is_some() {
                authority.to_owned()
            } else {
                format!("{authority}:80")
            };
            let stream = tokio::net::TcpStream::connect(address)
                .await
                .map_err(|e| format!("MCP HTTP connect failed: {e}"))?;
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream))
                    .await
                    .map_err(|e| e.to_string())?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let path = self.uri.path_and_query().map_or("/mcp", |p| {
                if p.as_str() == "/" {
                    "/mcp"
                } else {
                    p.as_str()
                }
            });
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("host", authority)
                .header("content-type", "application/json")
                .header("accept", "application/json");
            if let Some(token) = &self.token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            if let Some(session) = &self.session {
                request = request.header("mcp-session-id", session);
            }
            let bytes = body
                .map(|v| serde_json::to_vec(&v))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let response = sender
                .send_request(
                    request
                        .body(Full::new(Bytes::from(bytes)))
                        .map_err(|e| e.to_string())?,
                )
                .await
                .map_err(|e| e.to_string())?;
            if let Some(id) = response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
            {
                self.session = Some(id.into());
            }
            let status = response.status();
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| e.to_string())?
                .to_bytes();
            if !status.is_success() {
                return Err(format!("MCP HTTP request failed with status {status}"));
            }
            if bytes.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_slice(&bytes).map_err(|e| format!("invalid MCP HTTP JSON: {e}"))
            }
        })
        .await
        .map_err(|_| "MCP HTTP request timed out".to_owned())?
    }
}

#[async_trait]
impl Mcp for HttpMcp {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.exchange(
            Method::POST,
            Some(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})),
        )
        .await
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.exchange(
            Method::POST,
            Some(json!({"jsonrpc":"2.0","method":method,"params":params})),
        )
        .await
        .map(|_| ())
    }
    async fn close(&mut self) -> Result<()> {
        if self.session.is_some() {
            self.exchange(Method::DELETE, None).await?;
            self.session = None;
        }
        Ok(())
    }
}

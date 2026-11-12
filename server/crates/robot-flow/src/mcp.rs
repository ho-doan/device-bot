//! Portable, single-session MCP stdio client. No platform driver dependencies.
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub type Result<T> = std::result::Result<T, String>;

#[async_trait]
pub trait Mcp: Send {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value>;
    async fn notify(&mut self, method: &str, params: Value) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
}

pub struct StdioMcp {
    child: Child,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    pending: BTreeSet<u64>,
    timeout: Duration,
    poisoned: bool,
}

impl StdioMcp {
    pub fn spawn(program: &str, args: &[String], timeout: Duration) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Tool failures arrive through JSON-RPC. Do not persist unredacted
            // server stderr (which may include application credentials).
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("start MCP server: {e}"))?;
        Ok(Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().ok_or("missing MCP stdout")?).lines(),
            child,
            next_id: 1,
            pending: BTreeSet::new(),
            timeout,
            poisoned: false,
        })
    }

    async fn send(&mut self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let input = self.input.as_mut().ok_or("MCP session closed")?;
        input.write_all(&bytes).await.map_err(|e| e.to_string())?;
        input.flush().await.map_err(|e| e.to_string())
    }
}

#[async_trait]
impl Mcp for StdioMcp {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        if self.poisoned {
            return Err("MCP session is unusable after timeout or transport error".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id);
        let limit = self.timeout;
        let result = timeout(limit, async {
            self.send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
                .await?;
            loop {
                // Lines keeps its partial buffer across outer step cancellation;
                // read_line with a stack-local String loses that prefix.
                let line = self
                    .output
                    .next_line()
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("MCP EOF while awaiting {method}"))?;
                if line.len() > 32 * 1024 * 1024 {
                    return Err("MCP message exceeds 32 MiB".into());
                }
                let value: Value =
                    serde_json::from_str(&line).map_err(|e| format!("invalid MCP JSON: {e}"))?;
                if value.get("id").is_none() && value.get("method").is_some() {
                    continue;
                }
                if value.get("id") != Some(&json!(id)) {
                    // Outer step cancellation may leave an earlier request's
                    // response unread. Drain only replies to known issued IDs;
                    // unrelated or future IDs remain protocol errors.
                    if let Some(previous) = value.get("id").and_then(Value::as_u64) {
                        if previous < id && self.pending.remove(&previous) {
                            continue;
                        }
                    }
                    return Err("MCP response ID mismatch".into());
                }
                self.pending.remove(&id);
                if let Some(error) = value.get("error") {
                    return Err(format!("MCP protocol error: {error}"));
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or("MCP result missing".into());
            }
        })
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                self.poisoned = true;
                Err(error)
            }
            Err(_) => {
                self.poisoned = true;
                Err(format!("MCP timeout awaiting {method}"))
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        timeout(
            self.timeout,
            self.send(json!({"jsonrpc":"2.0","method":method,"params":params})),
        )
        .await
        .map_err(|_| "MCP notification timeout".to_string())?
    }

    async fn close(&mut self) -> Result<()> {
        self.input.take();
        let status = match timeout(Duration::from_secs(7), self.child.wait()).await {
            Ok(result) => result.map_err(|e| e.to_string())?,
            Err(_) => {
                self.child.start_kill().map_err(|e| e.to_string())?;
                let _ = self.child.wait().await;
                return Err("MCP shutdown timeout; connection closed for context cleanup".into());
            }
        };
        if !status.success() {
            return Err(format!("MCP server exited with {status} during cleanup"));
        }

        Ok(())
    }
}

pub async fn initialize(client: &mut dyn Mcp) -> Result<BTreeMap<String, Value>> {
    let initialized = client
        .request(
            "initialize",
            json!({
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"robot-flow","version":env!("CARGO_PKG_VERSION")}
            }),
        )
        .await?;
    if initialized["protocolVersion"] != "2025-11-25" {
        return Err("unsupported MCP protocol version".into());
    }
    client
        .notify("notifications/initialized", json!({}))
        .await?;
    let mut tools = BTreeMap::new();
    let mut cursor = None;
    for _ in 0..100 {
        let page = client
            .request(
                "tools/list",
                cursor
                    .as_ref()
                    .map(|c| json!({"cursor":c}))
                    .unwrap_or(json!({})),
            )
            .await?;
        for tool in page["tools"].as_array().ok_or("MCP tools list missing")? {
            let name = tool["name"].as_str().ok_or("MCP tool name missing")?;
            if tools
                .insert(name.to_owned(), tool["inputSchema"].clone())
                .is_some()
            {
                return Err(format!("duplicate MCP tool {name}"));
            }
        }
        let next = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if next.is_none() {
            return Ok(tools);
        }
        if next == cursor {
            return Err("MCP tools cursor did not advance".into());
        }
        cursor = next;
    }
    Err("MCP tools pagination exceeded 100 pages".into())
}

/// Validate the subset of JSON Schema used by device-bot's tool contracts.
/// Unknown constraints are rejected instead of being silently ignored.
pub fn validate_arguments(schema: &Value, value: &Value) -> Result<()> {
    let keywords = schema
        .as_object()
        .ok_or("advertised MCP inputSchema must be an object")?;
    for key in keywords.keys() {
        if ![
            "$schema",
            "title",
            "description",
            "type",
            "properties",
            "required",
            "additionalProperties",
            "enum",
            "minimum",
            "maximum",
            "minLength",
            "maxLength",
            "minItems",
            "maxItems",
            "items",
            "format",
            "default",
            "examples",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unsupported MCP schema constraint {key}"));
        }
    }
    if schema.get("type").is_some_and(|t| !t.is_string()) {
        return Err("unsupported MCP schema type declaration".into());
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "null" => value.is_null(),
            _ => false,
        };
        if !valid {
            return Err(format!("MCP argument must be {kind}"));
        }
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
        if !choices.contains(value) {
            return Err("MCP argument is outside advertised enum".into());
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required {
            let key = key.as_str().ok_or("invalid required property")?;
            if value.get(key).is_none() {
                return Err(format!("missing MCP argument {key}"));
            }
        }
    }
    if let Some(object) = value.as_object() {
        for (key, item) in object {
            if let Some(property) = schema.get("properties").and_then(|p| p.get(key)) {
                validate_arguments(property, item).map_err(|e| format!("{key}: {e}"))?;
            } else if schema.get("additionalProperties") == Some(&json!(false)) {
                return Err(format!("unknown MCP argument {key}"));
            } else if let Some(s) = schema.get("additionalProperties").filter(|s| s.is_object()) {
                validate_arguments(s, item)?;
            }
        }
    }
    if let Some(items) = value.as_array() {
        for item in items {
            if let Some(s) = schema.get("items") {
                validate_arguments(s, item)?;
            }
        }
    }
    let size = value
        .as_str()
        .map(|s| s.chars().count())
        .or_else(|| value.as_array().map(Vec::len));
    for (keyword, count, minimum) in [
        ("minLength", size, true),
        ("maxLength", size, false),
        ("minItems", size, true),
        ("maxItems", size, false),
    ] {
        if let (Some(bound), Some(actual)) = (schema[keyword].as_u64(), count) {
            if (minimum && actual < bound as usize) || (!minimum && actual > bound as usize) {
                return Err(format!("MCP argument violates {keyword}"));
            }
        }
    }
    for (keyword, minimum) in [("minimum", true), ("maximum", false)] {
        if let (Some(bound), Some(actual)) = (schema[keyword].as_f64(), value.as_f64()) {
            if !actual.is_finite() || (minimum && actual < bound) || (!minimum && actual > bound) {
                return Err(format!("MCP argument violates {keyword}"));
            }
        }
    }
    for keyword in ["$ref", "oneOf", "anyOf", "allOf", "not", "pattern"] {
        if schema.get(keyword).is_some() {
            return Err(format!("unsupported MCP schema constraint {keyword}"));
        }
    }
    Ok(())
}

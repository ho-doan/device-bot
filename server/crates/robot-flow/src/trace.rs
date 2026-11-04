//! Trace protocol boundaries without writing request arguments or native values.
use crate::mcp::{Mcp, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub struct TracedMcp {
    inner: Box<dyn Mcp>,
    path: PathBuf,
    sequence: u64,
}
impl TracedMcp {
    pub fn new(inner: Box<dyn Mcp>, path: PathBuf) -> Self {
        Self {
            inner,
            path,
            sequence: 0,
        }
    }
    fn log(&mut self, method: &str, tool: Option<&str>, direction: &str) -> Result<()> {
        self.sequence += 1;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| e.to_string())?;
        writeln!(
            file,
            "{}",
            json!({"sequence":self.sequence,"method":method,"tool":tool,"direction":direction})
        )
        .map_err(|e| e.to_string())
    }
}
#[async_trait]
impl Mcp for TracedMcp {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let tool = params
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.log(method, tool.as_deref(), "request")?;
        let result = self.inner.request(method, params).await;
        self.log(
            method,
            tool.as_deref(),
            if result.is_ok() { "response" } else { "error" },
        )?;
        result
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.log(method, None, "notification")?;
        self.inner.notify(method, params).await
    }
    async fn close(&mut self) -> Result<()> {
        self.inner.close().await
    }
}

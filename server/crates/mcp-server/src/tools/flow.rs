//! Replay QC recordings with the same runner and MCP contract as the CLI.
use super::arguments::required_string;
use crate::Server;
use async_trait::async_trait;
use device_mcp_protocol::{JsonRpcRequest, ToolDefinition, ToolResult};
use robot_core::Robot;
use robot_flow::{
    FlowDocument, FlowRunner, Profile,
    mcp::{Mcp, Result},
    schema,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub(super) const NAME: &str = "mobile_run_flow";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(), title: Some("Run YAML scenario".into()),
        description: "Run a complete robot-flow YAML scenario in a separate case session. Returns PASS/FAIL, per-step results and server artifact directory. Does not record these replayed actions into the QC draft.".into(),
        input_schema: json!({"type":"object","properties":{"yaml":{"type":"string","minLength":1,"maxLength":1048576}},"required":["yaml"],"additionalProperties":false}),
    }
}

struct CaseMcp {
    server: Server,
}

#[async_trait]
impl Mcp for CaseMcp {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        if method == "tools/call" && params["name"] == NAME {
            return Err("A flow cannot recursively run another flow".into());
        }
        let response = self
            .server
            .handle(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: method.into(),
                params: Some(params),
            })
            .await
            .ok_or("MCP response missing")?;
        if let Some(error) = response.get("error") {
            return Err(error.to_string());
        }
        response
            .get("result")
            .cloned()
            .ok_or("MCP result missing".into())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.server
            .handle(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: method.into(),
                params: Some(params),
            })
            .await;
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.server.shutdown().await.map_err(|e| e.to_string())
    }
}

pub(super) async fn run(robot: &Arc<dyn Robot>, args: &Value) -> ToolResult {
    let yaml = match required_string(args, "yaml") {
        Ok(value) => value,
        Err(e) => return e,
    };
    if yaml.len() > 1024 * 1024 {
        return ToolResult::execution_error("YAML exceeds 1 MiB");
    }
    let flow = match FlowDocument::from_yaml(&yaml) {
        Ok(flow) => flow,
        Err(e) => return ToolResult::execution_error(e),
    };
    let profile = Profile::default();
    if let Err(e) = schema::compile(&flow, &profile) {
        return ToolResult::execution_error(e);
    }
    let directory = PathBuf::from(
        std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()),
    )
    .join("qc-flows")
    .join(uuid::Uuid::new_v4().to_string());
    if let Err(e) = std::fs::create_dir_all(&directory)
        .and_then(|_| std::fs::write(directory.join("scenario.yaml"), &yaml))
    {
        return ToolResult::execution_error(format!("Could not save scenario: {e}"));
    }
    // Chrome owns a disposable context per case; the live QC case stays intact.
    let case = robot.fork_session().unwrap_or_else(|| Arc::clone(robot));
    let client = CaseMcp {
        server: Server::new(case),
    };
    let mut runner = FlowRunner::new(Some(Box::new(client)), directory.clone());
    let report = runner
        .run(&flow, &profile, &[], Duration::from_secs(600))
        .await;
    let mut result = serde_json::to_value(report).expect("flow report");
    result["artifactDirectory"] = json!(directory.to_string_lossy());
    if let Err(error) = std::fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    ) {
        result["artifactWarning"] = json!(error.to_string());
    }
    ToolResult::structured(result)
}

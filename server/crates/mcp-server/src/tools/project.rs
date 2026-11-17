//! Session-owned named project jobs, executed by robot-project.
use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Duration};
use async_trait::async_trait;
use device_mcp_protocol::{JsonRpcRequest, ToolDefinition, ToolResult};
use robot_core::Robot;
use robot_flow::mcp::{Mcp, Result};
use robot_project::{Project, plan::{Host, HostBinding, Resource}, session::{DeviceBotFactory, SessionFactory}};
use serde_json::{json, Value};
use tokio::{sync::watch, task::JoinHandle};
use crate::Server;

pub(super) const VALIDATE: &str = "project_validate";
pub(super) const START: &str = "project_start";
pub(super) const STATUS: &str = "project_status";
pub(super) const CANCEL: &str = "project_cancel";

pub(super) fn definitions() -> Vec<ToolDefinition> {
    [(VALIDATE, "Validate a named project without controlling devices", "path"), (START, "Start a named project job", "path"), (STATUS, "Read a project job", "runId"), (CANCEL, "Request cancellation and cleanup of a project job", "runId")].into_iter().map(|(name, description, field)| ToolDefinition {
        name: name.into(), title: None, description: description.into(),
        input_schema: if field == "path" { json!({"type":"object","properties":{"path":{"type":"string","minLength":1,"maxLength":4096},"confirm":{"type":"array","items":{"type":"string","maxLength":1024},"maxItems":100}},"required":["path"],"additionalProperties":false}) } else { json!({"type":"object","properties":{"runId":{"type":"string","minLength":1,"maxLength":128}},"required":["runId"],"additionalProperties":false}) },
    }).collect()
}
struct EmbeddedMcp(Server);
#[async_trait]
impl Mcp for EmbeddedMcp {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        if method == "tools/call" && params["name"].as_str().is_some_and(|n| n == "mobile_run_flow" || n.starts_with("project_")) {
            return Err("recursive flow or project execution is forbidden".into());
        }
        let response = Box::pin(self.0.handle(JsonRpcRequest { jsonrpc: "2.0".into(), id: Some(json!(1)), method: method.into(), params: Some(params) })).await.ok_or("MCP response missing")?;
        if response.get("error").is_some() { return Err("embedded MCP request failed".into()); }
        response.get("result").cloned().ok_or("MCP result missing".into())
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        Box::pin(self.0.handle(JsonRpcRequest { jsonrpc: "2.0".into(), id: None, method: method.into(), params: Some(params) })).await;
        Ok(())
    }
    async fn close(&mut self) -> Result<()> { Box::pin(self.0.shutdown()).await.map_err(|e| e.to_string().into()) }
}
struct EmbeddedFactory { robot: Arc<dyn Robot>, artifacts: PathBuf }
#[async_trait]
impl SessionFactory for EmbeddedFactory {
    async fn connect(&self, project: &Project, host: &Host, resource: &Resource) -> Result<Box<dyn Mcp>> {
        match &host.binding {
            HostBinding::Local {} => {
                let current = if cfg!(target_os = "macos") { robot_project::plan::HostPlatform::Macos } else if cfg!(target_os = "windows") { robot_project::plan::HostPlatform::Windows } else { robot_project::plan::HostPlatform::Linux };
                if host.platform != current { return Err("local host platform does not match this machine".into()); }
                let robot = self.robot.fork_session().ok_or("driver cannot create an isolated project session")?;
                Ok(Box::new(EmbeddedMcp(Server::new(robot))))
            }
            HostBinding::Mcp { .. } => DeviceBotFactory { server: PathBuf::new(), rpc_timeout: Duration::from_secs(180), artifacts_root: self.artifacts.clone() }.connect(project, host, resource).await,
        }
    }
}
struct Job { state: Arc<Mutex<Value>>, cancel: watch::Sender<bool>, task: JoinHandle<()> }
#[derive(Default)]
pub(super) struct Jobs { jobs: Mutex<HashMap<String, Job>> }
impl Jobs {
    pub(super) async fn shutdown(&self) {
        let jobs = { let mut jobs = self.jobs.lock().unwrap(); std::mem::take(&mut *jobs) };
        for job in jobs.values() { let _ = job.cancel.send(true); }
        for (_, job) in jobs { let _ = job.task.await; }
    }
    pub(super) async fn call(&self, robot: &Arc<dyn Robot>, name: &str, args: &Value) -> ToolResult {
        if name == STATUS || name == CANCEL {
            let Some(id) = args["runId"].as_str() else { return ToolResult::execution_error("runId is required"); };
            let jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get(id) else { return ToolResult::execution_error("unknown project run ID"); };
            let mut state = job.state.lock().unwrap();
            if name == CANCEL && ["running", "queued"].contains(&state["status"].as_str().unwrap_or("")) {
                state["status"] = json!("cancelling");
                let _ = job.cancel.send(true);
            }
            return ToolResult::structured(state.clone());
        }
        let Some(path) = args["path"].as_str().filter(|s| !s.trim().is_empty() && s.len() <= 4096) else { return ToolResult::execution_error("backend project path is required"); };
        let project = match Project::load(Path::new(path)) { Ok(p) => p, Err(_) => return ToolResult::execution_error("could not load project manifest") };
        if project.plan.is_legacy() { return ToolResult::execution_error("QC project jobs require named resources"); }
        let confirmed = match args.get("confirm") { None => Vec::new(), Some(Value::Array(items)) if items.len() <= 100 && items.iter().all(|v| v.as_str().is_some_and(|s| s.len() <= 1024)) => items.iter().map(|v| v.as_str().unwrap().to_owned()).collect(), _ => return ToolResult::execution_error("invalid precondition confirmations") };
        let count = match robot_project::session::validate(&project, &confirmed) { Ok(n) => n, Err(_) => return ToolResult::execution_error("project validation failed; check manifest, flows, profiles and preconditions") };
        if name == VALIDATE { return ToolResult::structured(json!({"status":"validated","caseCount":count,"message":"Dry run only; no device coverage"})); }
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.len() >= 32 { return ToolResult::execution_error("project job limit reached for this session"); }
        let id = uuid::Uuid::new_v4().to_string();
        let output = project.root.join("artifacts/project").join(format!("qc-{id}"));
        if std::fs::create_dir_all(&output).is_err() { return ToolResult::execution_error("could not create project evidence directory"); }
        let state = Arc::new(Mutex::new(json!({"runId":id,"status":"running","caseCount":count,"artifactDirectory":output.to_string_lossy()})));
        let (cancel, receiver) = watch::channel(false);
        let task_state = Arc::clone(&state);
        let robot = Arc::clone(robot);
        let task = tokio::spawn(async move {
            let factory = EmbeddedFactory { robot, artifacts: output.join("resources") };
            let result = robot_project::session::run_with_history_cancel(&project, &factory, &output, &confirmed, Duration::from_secs(600), None, receiver.clone()).await;
            let mut state = task_state.lock().unwrap();
            match result {
                Ok(result) => { state["status"] = json!(if result.reports.iter().any(|r| r.error.is_some()) { "failed" } else { "succeeded" }); state["reports"] = json!(result.reports.iter().take(100).map(|report| json!({"status":report.status,"steps":report.steps.len()})).collect::<Vec<_>>()); }
                Err(error) if error.kind() == robot_flow::outcome::ErrorKind::Interrupted => { state["status"] = json!("cancelled"); state["message"] = json!("Cancellation and cleanup completed."); }
                Err(_) => { state["status"] = json!("failed"); state["message"] = json!("Project execution failed; inspect backend evidence."); }
            }
        });
        let response = state.lock().unwrap().clone();
        jobs.insert(id, Job { state, cancel, task });
        ToolResult::structured(response)
    }
}

//! Session-owned named project jobs, executed by robot-project.
use crate::Server;
use async_trait::async_trait;
use device_mcp_protocol::{JsonRpcRequest, ToolDefinition, ToolResult};
use robot_core::Robot;
use robot_flow::mcp::{Mcp, Result};
use robot_project::{
    Project,
    plan::{Host, HostBinding, Resource},
    session::{DeviceBotFactory, SessionFactory},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

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
        if method == "tools/call"
            && params["name"]
                .as_str()
                .is_some_and(|n| n == "mobile_run_flow" || n.starts_with("project_"))
        {
            return Err("recursive flow or project execution is forbidden".into());
        }
        let response = Box::pin(self.0.handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params: Some(params),
        }))
        .await
        .ok_or("MCP response missing")?;
        if response.get("error").is_some() {
            return Err("embedded MCP request failed".into());
        }
        response
            .get("result")
            .cloned()
            .ok_or("MCP result missing".into())
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        Box::pin(self.0.handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: method.into(),
            params: Some(params),
        }))
        .await;
        Ok(())
    }
    async fn close(&mut self) -> Result<()> {
        Box::pin(self.0.shutdown())
            .await
            .map_err(|e| e.to_string().into())
    }
}
struct EmbeddedFactory {
    robot: Arc<dyn Robot>,
    artifacts: PathBuf,
}
#[async_trait]
impl SessionFactory for EmbeddedFactory {
    async fn connect(
        &self,
        project: &Project,
        host: &Host,
        resource: &Resource,
    ) -> Result<Box<dyn Mcp>> {
        match &host.binding {
            HostBinding::Local {} => {
                let current = if cfg!(target_os = "macos") {
                    robot_project::plan::HostPlatform::Macos
                } else if cfg!(target_os = "windows") {
                    robot_project::plan::HostPlatform::Windows
                } else {
                    robot_project::plan::HostPlatform::Linux
                };
                if host.platform != current {
                    return Err("local host platform does not match this machine".into());
                }
                let robot = self
                    .robot
                    .fork_session()
                    .ok_or("driver cannot create an isolated project session")?;
                Ok(Box::new(EmbeddedMcp(Server::new(robot))))
            }
            HostBinding::Mcp { .. } => {
                DeviceBotFactory {
                    server: PathBuf::new(),
                    rpc_timeout: Duration::from_secs(180),
                    artifacts_root: self.artifacts.clone(),
                }
                .connect(project, host, resource)
                .await
            }
        }
    }
}
struct Job {
    state: Arc<Mutex<Value>>,
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}
#[derive(Default)]
pub(super) struct Jobs {
    jobs: Mutex<HashMap<String, Job>>,
}
impl Jobs {
    pub(super) async fn shutdown(&self) {
        let jobs = {
            let mut jobs = self.jobs.lock().unwrap();
            std::mem::take(&mut *jobs)
        };
        for job in jobs.values() {
            let _ = job.cancel.send(true);
        }
        for (_, job) in jobs {
            let _ = job.task.await;
        }
    }
    pub(super) async fn call(
        &self,
        robot: &Arc<dyn Robot>,
        name: &str,
        args: &Value,
    ) -> ToolResult {
        if name == STATUS || name == CANCEL {
            let Some(id) = args["runId"].as_str() else {
                return ToolResult::execution_error("runId is required");
            };
            let jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get(id) else {
                return ToolResult::execution_error("unknown project run ID");
            };
            let mut state = job.state.lock().unwrap();
            if name == CANCEL
                && ["running", "queued"].contains(&state["status"].as_str().unwrap_or(""))
            {
                state["status"] = json!("cancelling");
                let _ = job.cancel.send(true);
            }
            return ToolResult::structured(state.clone());
        }
        let Some(path) = args["path"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
        else {
            return ToolResult::execution_error("backend project path is required");
        };
        let project = match Project::load(Path::new(path)) {
            Ok(p) => p,
            Err(_) => return ToolResult::execution_error("could not load project manifest"),
        };
        if project.plan.is_legacy() {
            return ToolResult::execution_error("QC project jobs require named resources");
        }
        let confirmed = match args.get("confirm") {
            None => Vec::new(),
            Some(Value::Array(items))
                if items.len() <= 100
                    && items
                        .iter()
                        .all(|v| v.as_str().is_some_and(|s| s.len() <= 1024)) =>
            {
                items
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect()
            }
            _ => return ToolResult::execution_error("invalid precondition confirmations"),
        };
        let count = match robot_project::session::validate(&project, &confirmed) {
            Ok(n) => n,
            Err(_) => {
                return ToolResult::execution_error(
                    "project validation failed; check manifest, flows, profiles and preconditions",
                );
            }
        };
        if name == VALIDATE {
            return ToolResult::structured(
                json!({"status":"validated","caseCount":count,"message":"Dry run only; no device coverage"}),
            );
        }
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.len() >= 32 {
            return ToolResult::execution_error("project job limit reached for this session");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let output = project
            .root
            .join("artifacts/project")
            .join(format!("qc-{id}"));
        if std::fs::create_dir_all(&output).is_err() {
            return ToolResult::execution_error("could not create project evidence directory");
        }
        let state = Arc::new(Mutex::new(
            json!({"runId":id,"status":"running","caseCount":count,"artifactDirectory":output.to_string_lossy()}),
        ));
        let (cancel, receiver) = watch::channel(false);
        let task_state = Arc::clone(&state);
        let robot = Arc::clone(robot);
        let task = tokio::spawn(async move {
            let factory = EmbeddedFactory {
                robot,
                artifacts: output.join("resources"),
            };
            let result = robot_project::session::run_with_history_cancel(
                &project,
                &factory,
                &output,
                &confirmed,
                Duration::from_secs(600),
                None,
                receiver.clone(),
            )
            .await;
            let mut state = task_state.lock().unwrap();
            match result {
                Ok(result) => {
                    state["status"] = json!(if result.reports.iter().any(|r| r.error.is_some()) {
                        "failed"
                    } else {
                        "succeeded"
                    });
                    state["reports"] = json!(
                        result
                            .reports
                            .iter()
                            .take(100)
                            .map(
                                |report| json!({"status":report.status,"steps":report.steps.len()})
                            )
                            .collect::<Vec<_>>()
                    );
                }
                Err(error) if error.kind() == robot_flow::outcome::ErrorKind::Interrupted => {
                    state["status"] = json!("cancelled");
                    state["message"] = json!("Cancellation and cleanup completed.");
                }
                Err(_) => {
                    state["status"] = json!("failed");
                    state["message"] = json!("Project execution failed; inspect backend evidence.");
                }
            }
        });
        let response = state.lock().unwrap().clone();
        jobs.insert(
            id,
            Job {
                state,
                cancel,
                task,
            },
        );
        ToolResult::structured(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use device_mcp_protocol::PROTOCOL_VERSION;

    struct ProjectRobot;
    #[async_trait]
    impl Robot for ProjectRobot {
        async fn list_devices(
            &self,
        ) -> std::result::Result<Vec<robot_core::RobotDevice>, robot_core::RobotError> {
            Ok(vec![robot_core::RobotDevice {
                id: "chrome:project-test".into(),
                name: "Project test browser".into(),
                platform: robot_core::Platform::Chrome,
                state: robot_core::DeviceState::Available,
            }])
        }

        fn fork_session(&self) -> Option<Arc<dyn Robot>> {
            Some(Arc::new(Self))
        }

        async fn take_screenshot(
            &self,
            _device_id: &str,
        ) -> std::result::Result<Vec<u8>, robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn tap(
            &self,
            _device_id: &str,
            _point: robot_core::Point,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn launch_app(
            &self,
            _device_id: &str,
            _app: &str,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn open_url(
            &self,
            _device_id: &str,
            _url: &str,
            _app: Option<&str>,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn type_text(
            &self,
            _device_id: &str,
            _text: &str,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn swipe(
            &self,
            _device_id: &str,
            _start: robot_core::Point,
            _end: robot_core::Point,
            _duration_ms: Option<u64>,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn install_app(
            &self,
            _device_id: &str,
            _path: &str,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn uninstall_app(
            &self,
            _device_id: &str,
            _app: &str,
        ) -> std::result::Result<(), robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }

        async fn list_elements(
            &self,
            _device_id: &str,
        ) -> std::result::Result<Vec<robot_core::UiElement>, robot_core::RobotError> {
            Err(robot_core::RobotError::CommandFailed(
                "unexpected call".into(),
            ))
        }
    }

    struct ProjectDir(PathBuf);
    impl ProjectDir {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("device-bot-mcp-project-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let host = if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(target_os = "windows") {
                "windows"
            } else {
                "linux"
            };
            std::fs::write(
                root.join("project.yaml"),
                format!("version: 1\nkind: project\nname: mcp-project\nhosts:\n  - {{name: desktop, platform: {host}, binding: {{kind: local}}}}\nresources:\n  - {{name: browser, kind: chrome, host: desktop, display: {{host: desktop}}, prepare: {{kind: attach}}, timeout_ms: 1000}}\nflows:\n  - path: flow.yaml\n    bindings: [{{target: browser}}]\n"),
            )
            .unwrap();
            std::fs::write(
                root.join("flow.yaml"),
                "version: 2\nname: smoke\nsteps:\n  - wait: {ms: 0}\n",
            )
            .unwrap();
            Self(root)
        }
    }
    impl Drop for ProjectDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn request(id: u64, method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(id)),
            method: method.into(),
            params: Some(params),
        }
    }

    #[tokio::test]
    async fn project_jobs_are_exposed_through_mcp_and_execute_shared_project_runtime() {
        let project = ProjectDir::new();
        let robot: Arc<dyn Robot> = Arc::new(ProjectRobot);
        let mut server = Server::new(robot);

        let initialize = server
            .handle(request(
                1,
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "project-contract-test", "version": "1"}
                }),
            ))
            .await
            .unwrap();
        assert!(initialize.get("error").is_none());
        assert!(
            server
                .handle(JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: None,
                    method: "notifications/initialized".into(),
                    params: Some(json!({})),
                })
                .await
                .is_none()
        );

        let listed = server
            .handle(request(2, "tools/list", json!({})))
            .await
            .unwrap();
        let tools = listed["result"]["tools"].as_array().unwrap();
        for name in [VALIDATE, START, STATUS, CANCEL] {
            assert!(tools.iter().any(|tool| tool["name"] == name));
        }

        let validated = server
            .handle(request(
                3,
                "tools/call",
                json!({
                    "name": VALIDATE,
                    "arguments": {"path": project.0}
                }),
            ))
            .await
            .unwrap();
        assert!(validated.get("error").is_none());
        assert_eq!(
            validated["result"]["structuredContent"]["status"],
            "validated"
        );
        assert_eq!(validated["result"]["structuredContent"]["caseCount"], 1);

        let started = server
            .handle(request(
                4,
                "tools/call",
                json!({"name": START, "arguments": {"path": project.0}}),
            ))
            .await
            .unwrap();
        assert!(started.get("error").is_none());
        assert!(
            ["running", "succeeded"].contains(
                &started["result"]["structuredContent"]["status"]
                    .as_str()
                    .unwrap_or_default()
            )
        );
        let run_id = started["result"]["structuredContent"]["runId"]
            .as_str()
            .unwrap()
            .to_owned();

        let mut final_state = Value::Null;
        for id in 5..105 {
            let status = server
                .handle(request(
                    id,
                    "tools/call",
                    json!({"name": STATUS, "arguments": {"runId": run_id}}),
                ))
                .await
                .unwrap();
            final_state = status["result"]["structuredContent"].clone();
            if final_state["status"] == "succeeded" || final_state["status"] == "failed" {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(final_state["status"], "succeeded");
        server.shutdown().await.unwrap();
    }
}

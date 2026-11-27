//! Project-lifetime MCP sessions and named device routing.
use crate::{
    Project,
    plan::{Capability, Host, HostBinding, HostPlatform, Preparation, Resource, ResourceKind},
    recording::{self, Recorder, SharedRecorder, StartPhase},
};
use async_trait::async_trait;
use robot_flow::{
    FlowDocument, FlowReport, Profile, Profiles,
    http_mcp::HttpMcp,
    mcp::{self, Mcp, StdioMcp},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, watch};

/// Factory seam for creating one isolated MCP connection per resource.
#[async_trait]
pub trait SessionFactory: Send + Sync {
    async fn connect(
        &self,
        project: &Project,
        host: &Host,
        resource: &Resource,
    ) -> mcp::Result<Box<dyn Mcp>>;
}

pub struct DeviceBotFactory {
    pub server: PathBuf,
    pub rpc_timeout: Duration,
    pub artifacts_root: PathBuf,
}
#[async_trait]
impl SessionFactory for DeviceBotFactory {
    async fn connect(
        &self,
        project: &Project,
        host: &Host,
        resource: &Resource,
    ) -> mcp::Result<Box<dyn Mcp>> {
        match &host.binding {
            HostBinding::Local {} => {
                let current = if cfg!(target_os = "macos") {
                    HostPlatform::Macos
                } else if cfg!(target_os = "windows") {
                    HostPlatform::Windows
                } else {
                    HostPlatform::Linux
                };
                if host.platform != current {
                    return Err("local host platform does not match this machine".into());
                }
                let server = self.server.to_str().ok_or("invalid server path")?;
                let mut environment = project.environment();
                let resource_artifacts = self.artifacts_root.join(&resource.name);
                environment.insert(
                    "DEVICE_BOT_ARTIFACT_DIR".into(),
                    resource_artifacts.to_string_lossy().into_owned(),
                );
                // Each owned stdio backend starts its own bridge listeners.
                // Avoid collisions unless the user explicitly configured a bind.
                environment
                    .entry("DEVICE_BOT_IOS_AGENT_BIND".into())
                    .or_insert_with(|| "127.0.0.1:0".into());
                environment
                    .entry("DEVICE_BOT_IOS_SIM_AGENT_BIND".into())
                    .or_insert_with(|| "127.0.0.1:0".into());
                Ok(Box::new(StdioMcp::spawn_with_environment(
                    server,
                    &["mcp".into()],
                    self.rpc_timeout,
                    &environment,
                )?))
            }
            HostBinding::Mcp {
                endpoint,
                allow_remote,
                token_env,
            } => {
                let url = project
                    .resolve(endpoint)
                    .map_err(|_| "cannot resolve MCP endpoint")?;
                let token = token_env
                    .as_ref()
                    .map(|name| {
                        project
                            .environment_value(name)
                            .ok_or("missing MCP token environment variable")
                    })
                    .transpose()?;
                Ok(Box::new(HttpMcp::new_explicit(
                    &url,
                    self.rpc_timeout,
                    token,
                    *allow_remote,
                )?))
            }
        }
    }
}

struct Case {
    flow: FlowDocument,
    profile: Profile,
    target: String,
}
struct Connected {
    client: Box<dyn Mcp>,
    real_id: String,
    device: Value,
}
type Shared = Arc<Mutex<BTreeMap<String, Connected>>>;

async fn close_owned(sessions: Shared) -> bool {
    let mut guard = sessions.lock().await;
    let mut clean = true;
    for connection in guard.values_mut() {
        if !matches!(
            tokio::time::timeout(Duration::from_secs(10), connection.client.close()).await,
            Ok(Ok(()))
        ) {
            clean = false;
        }
    }
    guard.clear();
    clean
}

// If a caller drops the project future, still schedule closure of its own
// sessions. Owned stdio children also have kill_on_drop as a last resort.
struct CleanupGuard {
    sessions: Option<Shared>,
    recorder: Option<SharedRecorder>,
}
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let sessions = self.sessions.take();
            let recorder = self.recorder.take();
            handle.spawn(async move {
                if let Some(recorder) = recorder {
                    let _ = recording::finish(&recorder).await;
                }
                if let Some(sessions) = sessions {
                    close_owned(sessions).await;
                }
            });
        }
    }
}

struct Router {
    sessions: Shared,
    devices: BTreeMap<String, Value>,
    tools: BTreeMap<String, Value>,
}
#[async_trait]
impl Mcp for Router {
    async fn request(&mut self, method: &str, mut params: Value) -> mcp::Result<Value> {
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => Ok(
                json!({"tools":self.tools.iter().map(|(name,schema)| json!({"name":name,"inputSchema":schema})).collect::<Vec<_>>()}),
            ),
            "tools/call" => {
                let name = params["name"].as_str().ok_or("tool name missing")?;
                if name == "mobile_list_available_devices" {
                    return Ok(
                        json!({"structuredContent":{"devices":self.devices.values().collect::<Vec<_>>()}}),
                    );
                }
                let args = params
                    .get_mut("arguments")
                    .and_then(Value::as_object_mut)
                    .ok_or("tool arguments missing")?;
                let alias = args
                    .get("deviceId")
                    .and_then(Value::as_str)
                    .ok_or("project tool calls require an explicitly routed deviceId")?
                    .to_owned();
                let mut sessions = self.sessions.lock().await;
                let session = sessions
                    .get_mut(&alias)
                    .ok_or("unknown project device alias")?;
                args.insert("deviceId".into(), json!(session.real_id));
                session.client.request(method, params).await
            }
            _ => Err("unsupported project session method".into()),
        }
    }
    async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
        Ok(())
    }
    async fn close(&mut self) -> mcp::Result<()> {
        Ok(())
    }
}

/// Results of a named project session; cases run serially and stop on failure.
pub struct SessionResult {
    pub reports: Vec<FlowReport>,
}

fn compile_cases(
    project: &Project,
    environment: &BTreeMap<String, String>,
    confirmed: &[String],
) -> mcp::Result<Vec<Case>> {
    let mut cases = Vec::new();
    for flow in &project.plan.flows {
        if flow.bindings.is_empty() {
            return Err("legacy and named project flows cannot be mixed".into());
        }
        let document = FlowDocument::from_path(&flow.path)?;
        let profiles = flow
            .profiles
            .as_deref()
            .map(Profiles::from_path)
            .transpose()?;
        for binding in &flow.bindings {
            let mut profile = match &binding.profile {
                Some(name) => profiles
                    .as_ref()
                    .and_then(|p| p.targets.get(name))
                    .cloned()
                    .ok_or_else(|| format!("unknown project profile {name}"))?,
                None => Profile::default(),
            };
            let (case_target, resource_name) = match (&binding.target, &binding.actor) {
                (Some(target), None) => (target.clone(), target.as_str()),
                (None, Some(actor_name)) => {
                    let actor = project
                        .plan
                        .actors
                        .iter()
                        .find(|actor| actor.name == *actor_name)
                        .ok_or("unknown project actor")?;
                    (actor.name.clone(), actor.resource.as_str())
                }
                _ => return Err("flow binding must specify exactly one of target or actor".into()),
            };
            let resource = project
                .plan
                .resources
                .iter()
                .find(|resource| resource.name == resource_name)
                .ok_or("unknown project target")?;
            if !profile.device.is_object() {
                profile.device = json!({});
            }
            profile.device["id"] = json!(format!("project:{}", resource.name));
            profile.device["platform"] = json!(platform(resource.kind));
            robot_flow::schema::compile_with_environment(&document, &profile, Some(environment))
                .map_err(|error| sanitize_case_error(error, &document, &profile, environment))?;
            for condition in profile.preconditions.iter().chain(&document.preconditions) {
                if !confirmed.contains(condition) {
                    return Err("project precondition not confirmed".into());
                }
            }
            cases.push(Case {
                flow: document.clone(),
                profile,
                target: case_target,
            });
        }
    }
    Ok(cases)
}

fn named_resource_devices(
    project: &Project,
    resources: &BTreeMap<String, Value>,
) -> mcp::Result<BTreeMap<String, Value>> {
    let mut named = resources.clone();
    for actor in &project.plan.actors {
        let device = resources
            .get(&actor.resource)
            .ok_or("actor Chrome resource is not prepared")?;
        named.insert(actor.name.clone(), device.clone());
    }
    Ok(named)
}

fn sanitize_case_error(
    error: robot_flow::FlowError,
    flow: &FlowDocument,
    profile: &Profile,
    environment: &BTreeMap<String, String>,
) -> robot_flow::FlowError {
    let kind = error.kind();
    let message = robot_flow::runner::redact_environment_error(
        &error.to_string(),
        flow,
        profile,
        environment,
    );
    robot_flow::FlowError::new(kind, message)
}

fn platform(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Android => "android",
        ResourceKind::IosSimulator | ResourceKind::IosReal => "ios",
        ResourceKind::Chrome => "chrome",
        ResourceKind::Windows => "windows",
        ResourceKind::Macos => "macos",
    }
}
fn capability_tools(capability: Capability) -> &'static [&'static str] {
    match capability {
        Capability::Discovery => &["mobile_list_available_devices"],
        Capability::Launch => &["mobile_launch_app"],
        Capability::Input => &["mobile_click_on_screen_at_coordinates", "mobile_type_keys"],
        Capability::Screenshot => &["mobile_take_screenshot"],
        Capability::Elements => &["mobile_list_elements_on_screen"],
    }
}

/// Execute named cases through project-owned MCP connections. The factory can
/// be replaced by callers for tests; no global environment is mutated.
pub async fn run(
    project: &Project,
    factory: &dyn SessionFactory,
    directory: &Path,
    confirmed: &[String],
    case_timeout: Duration,
) -> mcp::Result<SessionResult> {
    run_with_history(project, factory, directory, confirmed, case_timeout, None).await
}

/// As `run`, additionally import each persisted case into report history.
pub async fn run_with_history(
    project: &Project,
    factory: &dyn SessionFactory,
    directory: &Path,
    confirmed: &[String],
    case_timeout: Duration,
    history_root: Option<&Path>,
) -> mcp::Result<SessionResult> {
    run_with_history_interruptible(
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
        interrupted(),
    )
    .await
}

/// As `run_with_history`, with cancellation supplied by an owning project job.
/// Owned MCP sessions are closed before this returns, including on cancellation.
pub async fn run_with_history_cancel(
    project: &Project,
    factory: &dyn SessionFactory,
    directory: &Path,
    confirmed: &[String],
    case_timeout: Duration,
    history_root: Option<&Path>,
    cancel: watch::Receiver<bool>,
) -> mcp::Result<SessionResult> {
    let cancelled = async move {
        let mut cancel = cancel;
        loop {
            if *cancel.borrow() {
                return mcp::FlowError::new(mcp::ErrorKind::Interrupted, "project cancelled");
            }
            if cancel.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    };
    run_with_history_interruptible(
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
        cancelled,
    )
    .await
}

async fn run_with_history_interruptible<F>(
    project: &Project,
    factory: &dyn SessionFactory,
    directory: &Path,
    confirmed: &[String],
    case_timeout: Duration,
    history_root: Option<&Path>,
    interrupt: F,
) -> mcp::Result<SessionResult>
where
    F: Future<Output = mcp::FlowError>,
{
    let environment = project.environment();
    let cases = compile_cases(project, &environment, confirmed)?;
    for resource in &project.plan.resources {
        if resource.display.via.is_some() {
            return Err("remote display preparation is unsupported".into());
        }
        if matches!(resource.prepare, Preparation::Launch { .. })
            && resource.kind != ResourceKind::Chrome
        {
            return Err(
                "native launch/boot requires an ownership-aware adapter; attach only".into(),
            );
        }
    }
    let sessions: Shared = Arc::new(Mutex::new(BTreeMap::new()));
    let recorder = project
        .recording
        .as_ref()
        .map(|_| Recorder::new(&directory.join("recording"), project));
    let mut cleanup = CleanupGuard {
        sessions: Some(Arc::clone(&sessions)),
        recorder: recorder.clone(),
    };
    let context = RunContext {
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
    };
    let execute = async {
        if let Some(recorder) = &recorder {
            recording::prepare(recorder, project, factory).await?;
            if project
                .recording
                .as_ref()
                .is_some_and(|config| config.start == StartPhase::BeforePrepare)
            {
                recording::start(recorder).await?;
            }
        }
        run_owned(
            context,
            environment,
            cases,
            Arc::clone(&sessions),
            recorder.clone(),
        )
        .await
    };
    let result = tokio::select! {
        result = execute => result,
        error = interrupt => Err(error),
    };
    let recording_clean = if let Some(recorder) = cleanup.recorder.take() {
        recording::finish(&recorder).await
    } else {
        Ok(())
    };
    let clean = close_owned(Arc::clone(&sessions)).await;
    cleanup.sessions = None;
    match result {
        Err(error) => Err(error),
        Ok(_) if !clean => Err("project MCP cleanup failed".into()),
        Ok(_) if recording_clean.is_err() => Err(recording_clean.unwrap_err()),
        Ok(result) => Ok(result),
    }
}

async fn interrupted() -> mcp::FlowError {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            return "project interrupted".into();
        }
    }
    let _ = tokio::signal::ctrl_c().await;
    "project interrupted".into()
}

/// Compile all cases without creating sessions or performing device actions.
pub fn validate(project: &Project, confirmed: &[String]) -> mcp::Result<usize> {
    compile_cases(project, &project.environment(), confirmed).map(|cases| cases.len())
}

async fn invoke(
    client: &mut dyn Mcp,
    name: &str,
    arguments: Value,
    limit: Duration,
) -> mcp::Result<Value> {
    tokio::time::timeout(limit, call_tool(client, name, arguments))
        .await
        .map_err(|_| {
            mcp::FlowError::new(
                mcp::ErrorKind::Timeout,
                format!("project tool {name} timed out"),
            )
        })?
}

fn proxy_configuration(config: &robot_core::WebProxyConfig) -> Value {
    json!({"deviceId":"chrome:browser","action":"configure","config": config})
}

async fn call_tool(client: &mut dyn Mcp, name: &str, arguments: Value) -> mcp::Result<Value> {
    let result = client
        .request("tools/call", json!({"name":name,"arguments":arguments}))
        .await?;
    if result["isError"] == true {
        return Err(format!("project tool {name} failed").into());
    }
    Ok(result)
}

async fn discover_device(client: &mut dyn Mcp, resource: &Resource) -> mcp::Result<Value> {
    const DISCOVERY_INTERVAL: Duration = Duration::from_millis(100);
    let wait_for_owned_page = resource.kind == ResourceKind::Chrome
        && matches!(resource.prepare, Preparation::Launch { .. });
    let limit = Duration::from_millis(resource.timeout_ms);
    tokio::time::timeout(limit, async {
        loop {
            let result = call_tool(client, "mobile_list_available_devices", json!({}))
                .await
                .map_err(|error| {
                    mcp::FlowError::new(
                        error.kind(),
                        format!("resource {} discovery request failed", resource.name),
                    )
                })?;
            let available = result
                .pointer("/structuredContent/devices")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    mcp::FlowError::new(
                        mcp::ErrorKind::Setup,
                        format!("resource {} discovery returned no device list", resource.name),
                    )
                })?;
            let matches = available.iter().filter(|device| {
                device["platform"] == platform(resource.kind)
                    && resource.selector.as_deref().is_none_or(|selector| {
                        device["id"].as_str() == Some(selector)
                            || device["name"].as_str() == Some(selector)
                    })
                    && device["state"] == "available"
            });
            let matched: Vec<_> = matches.collect();
            match matched.as_slice() {
                [device] => return Ok((*device).clone()),
                [] if wait_for_owned_page => tokio::time::sleep(DISCOVERY_INTERVAL).await,
                _ => {
                    return Err(mcp::FlowError::new(
                        mcp::ErrorKind::Setup,
                        format!(
                            "resource {} matched {} available devices; require exactly one",
                            resource.name,
                            matched.len()
                        ),
                    ));
                }
            }
        }
    })
    .await
    .map_err(|_| {
        mcp::FlowError::new(
            mcp::ErrorKind::Timeout,
            format!(
                "resource {} discovery timed out after {} ms while waiting for exactly one available device",
                resource.name, resource.timeout_ms
            ),
        )
    })?
}

mod execution;
use execution::{RunContext, run_owned};

#[cfg(test)]
mod tests;

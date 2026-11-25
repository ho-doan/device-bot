//! Project-lifetime MCP sessions and named device routing.
use crate::{
    Project,
    plan::{Capability, Host, HostBinding, HostPlatform, Preparation, Resource, ResourceKind},
    recording::{self, Recorder, SharedRecorder, StartPhase},
};
use async_trait::async_trait;
use robot_flow::{
    FlowDocument, FlowReport, FlowRunner, Profile, Profiles,
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

struct RunContext<'a> {
    project: &'a Project,
    factory: &'a dyn SessionFactory,
    directory: &'a Path,
    confirmed: &'a [String],
    case_timeout: Duration,
    history_root: Option<&'a Path>,
}

async fn run_owned(
    context: RunContext<'_>,
    environment: BTreeMap<String, String>,
    cases: Vec<Case>,
    sessions: Shared,
    recorder: Option<SharedRecorder>,
) -> mcp::Result<SessionResult> {
    let RunContext {
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
    } = context;
    let mut devices = BTreeMap::new();
    let mut common_tools: Option<BTreeMap<String, Value>> = None;
    let configs = project
        .plan
        .resources
        .iter()
        .map(|resource| {
            project
                .resolve_web_proxy(resource)
                .map(|config| (resource.name.clone(), config))
        })
        .collect::<std::io::Result<BTreeMap<_, _>>>()
        .map_err(|error| mcp::FlowError::new(mcp::ErrorKind::Setup, error.to_string()))?;
    // Connect and validate every tool/flow before any resource is launched.
    for resource in &project.plan.resources {
        let host = project
            .plan
            .hosts
            .iter()
            .find(|h| h.name == resource.host)
            .ok_or("unknown resource host")?;
        let client = tokio::time::timeout(
            Duration::from_millis(resource.timeout_ms),
            factory.connect(project, host, resource),
        )
        .await
        .map_err(|_| "project MCP connection timed out")??;
        let alias = format!("project:{}", resource.name);
        sessions.lock().await.insert(
            alias.clone(),
            Connected {
                client,
                real_id: String::new(),
                device: Value::Null,
            },
        );
        let mut guard = sessions.lock().await;
        let connection = guard.get_mut(&alias).unwrap();
        let tools = tokio::time::timeout(
            Duration::from_millis(resource.timeout_ms),
            mcp::initialize(connection.client.as_mut()),
        )
        .await
        .map_err(|_| "project MCP initialization timed out")??;
        for capability in &resource.requires {
            for name in capability_tools(*capability) {
                if !tools.contains_key(*name) {
                    return Err(format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                    .into());
                }
            }
        }
        if let Preparation::Launch { url: Some(url), .. } = &resource.prepare {
            let url = project
                .resolve(url)
                .map_err(|_| "cannot resolve Chrome launch URL")?;
            for (name, args) in [
                ("mobile_boot_device", json!({"deviceId":"chrome:browser"})),
                (
                    "mobile_open_url",
                    json!({"deviceId":"chrome:page", "url":url}),
                ),
            ] {
                let schema = tools.get(name).ok_or_else(|| {
                    format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                })?;
                mcp::validate_arguments(schema, &args)?;
            }
            for name in ["mobile_boot_device", "mobile_open_url"] {
                if !tools.contains_key(name) {
                    return Err(format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                    .into());
                }
            }
        }
        if let Some(config) = &configs[&resource.name] {
            let name = mcp::WEB_TEST_PROXY;
            let schema = tools.get(name).ok_or_else(|| {
                format!(
                    "resource {} requires unavailable tool {name}",
                    resource.name
                )
            })?;
            mcp::validate_arguments(schema, &proxy_configuration(config))?;
        }
        if !tools.contains_key("mobile_list_available_devices") {
            return Err("project resource discovery tool unavailable".into());
        }
        let capability_schema = tools
            .get("mobile_get_capabilities")
            .ok_or("project capability tool unavailable")?;
        mcp::validate_arguments(capability_schema, &json!({"deviceId":"project-preflight"}))?;
        devices.insert(
            resource.name.clone(),
            json!({"id":alias,"platform":platform(resource.kind)}),
        );
        if let Some(common) = &mut common_tools {
            common.retain(|name, schema| tools.get(name) == Some(schema));
        } else {
            common_tools = Some(tools);
        }
    }
    let tools = common_tools.unwrap_or_default();
    let named_devices = named_resource_devices(project, &devices)?;
    for case in &cases {
        let mut runner = FlowRunner::new(None, directory.to_owned());
        runner.set_environment(environment.clone());
        runner.set_named_resources(named_devices.clone());
        runner
            .preflight_case(&case.flow, &case.profile, tools.clone())
            .map_err(|error| sanitize_case_error(error, &case.flow, &case.profile, &environment))?;
    }
    // Discover attached targets before launching any owned Chrome contexts.
    for launching in [false, true] {
        for resource in &project.plan.resources {
            let is_launch = matches!(resource.prepare, Preparation::Launch { .. });
            if is_launch != launching {
                continue;
            }
            let alias = format!("project:{}", resource.name);
            let limit = Duration::from_millis(resource.timeout_ms);
            let mut guard = sessions.lock().await;
            let connection = guard.get_mut(&alias).unwrap();
            if is_launch {
                if let Some(config) = &configs[&resource.name] {
                    let configured = invoke(
                        connection.client.as_mut(),
                        mcp::WEB_TEST_PROXY,
                        proxy_configuration(config),
                        limit,
                    )
                    .await?;
                    if configured
                        .pointer("/structuredContent/configured")
                        .and_then(Value::as_bool)
                        != Some(true)
                    {
                        return Err(format!(
                            "resource {} driver did not confirm its test proxy",
                            resource.name
                        )
                        .into());
                    }
                }
                invoke(
                    connection.client.as_mut(),
                    "mobile_boot_device",
                    json!({"deviceId":"chrome:browser"}),
                    limit,
                )
                .await?;
            }
            let mut device = discover_device(connection.client.as_mut(), resource).await?;
            connection.real_id = device["id"]
                .as_str()
                .ok_or("discovered device ID missing")?
                .to_owned();
            let id = connection.real_id.clone();
            let result = invoke(
                connection.client.as_mut(),
                "mobile_get_capabilities",
                json!({"deviceId":id}),
                limit,
            )
            .await?;
            let report = result
                .get("structuredContent")
                .ok_or("project capability report missing")?;
            let host = project
                .plan
                .hosts
                .iter()
                .find(|host| host.name == resource.host)
                .ok_or("unknown project host")?;
            let expected_host = match host.platform {
                HostPlatform::Macos => "macos",
                HostPlatform::Windows => "windows",
                HostPlatform::Linux => "linux",
            };
            if report["hostOs"].as_str() != Some(expected_host) {
                return Err(format!(
                    "resource {} host OS does not match declared host",
                    resource.name
                )
                .into());
            }
            if report["deviceId"].as_str() != Some(id.as_str()) {
                return Err(format!(
                    "resource {} capability device identity mismatch",
                    resource.name
                )
                .into());
            }
            for capability in &resource.requires {
                let name = match capability {
                    Capability::Discovery => "discovery",
                    Capability::Launch => "launch",
                    Capability::Input => "input",
                    Capability::Screenshot => "screenshot",
                    Capability::Elements => "elements",
                };
                let status = report["capabilities"][name]["status"]
                    .as_str()
                    .unwrap_or("unknown");
                if status != "available" {
                    return Err(format!(
                        "resource {} requires {name}, capability status {}",
                        resource.name,
                        if ["blocked", "unsupported", "unknown"].contains(&status) {
                            status
                        } else {
                            "unknown"
                        }
                    )
                    .into());
                }
            }
            device["id"] = json!(alias);
            connection.device = device.clone();
            devices.insert(resource.name.clone(), device);
        }
    }
    for resource in &project.plan.resources {
        if let Preparation::Launch { url: Some(url), .. } = &resource.prepare {
            let url = project
                .resolve(url)
                .map_err(|_| "cannot resolve Chrome launch URL")?;
            let mut guard = sessions.lock().await;
            let connection = guard
                .get_mut(&format!("project:{}", resource.name))
                .unwrap();
            let id = connection.real_id.clone();
            invoke(
                connection.client.as_mut(),
                "mobile_open_url",
                json!({"deviceId":id,"url":url}),
                Duration::from_millis(resource.timeout_ms),
            )
            .await?;
        }
    }
    let named_devices = named_resource_devices(project, &devices)?;
    if project
        .recording
        .as_ref()
        .is_some_and(|config| config.start == StartPhase::AfterPrepare)
    {
        recording::start(recorder.as_ref().ok_or("recorder setup missing")?).await?;
    }
    let mut reports = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let default = named_devices
            .get(&case.target)
            .ok_or("project target is not prepared")?
            .clone();
        let router = Router {
            sessions: Arc::clone(&sessions),
            devices: devices.clone(),
            tools: tools.clone(),
        };
        let mut runner = FlowRunner::new(
            Some(Box::new(router)),
            directory.join(format!("case-{:03}", index + 1)),
        );
        runner.set_environment(environment.clone());
        runner.set_named_resources(named_devices.clone());
        runner.set_default_device(default);
        if let Some(recorder) = &recorder {
            if let Some(sink) = recorder
                .lock()
                .await
                .step_sink(index + 1, case.target.clone())
            {
                runner.set_step_event_sink(sink);
            }
        }
        let mut report = runner
            .run(&case.flow, &case.profile, confirmed, case_timeout)
            .await;
        if let Err(error) = runner.annotate_video(&report).await {
            report.status = "FAIL".into();
            if let Some(existing) = &mut report.error {
                existing.push_str(&format!("; {error}"));
            } else {
                report.error = Some(error.to_string());
                report.error_kind = Some("artifact".into());
            }
        }
        let case_dir = directory.join(format!("case-{:03}", index + 1));
        robot_flow::report::save_report(&report, &case_dir)?;
        let snapshot = case_dir.join("expanded-flow.yaml");
        if snapshot.is_file() {
            std::fs::copy(snapshot, case_dir.join("scenario.yaml"))
                .map_err(|error| error.to_string())?;
        }
        if let Some(root) = history_root {
            let history = robot_report::Store::open(root).and_then(|store| {
                store.ingest_flow(robot_report::FlowIngest {
                    name: report.name.clone(),
                    source: "cli".into(),
                    case_dir: case_dir.clone(),
                    app_id: Some(case.profile.app_id.clone())
                        .filter(|value| !value.trim().is_empty()),
                    url: None,
                })
            });
            if history.is_err() {
                eprintln!("History not updated for project case");
            }
        }
        let failed = report.status == "FAIL";
        reports.push(report);
        if failed {
            break;
        }
    }
    Ok(SessionResult { reports })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new(flow: &str, profiles: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("device-project-session-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("project.yaml"),
                r#"
version: 1
kind: project
name: test
hosts:
  - {name: local, platform: macos, binding: {kind: local}}
  - {name: other, platform: macos, binding: {kind: local}}
resources:
  - name: a
    kind: chrome
    host: local
    display: {host: local}
    prepare: {kind: launch, url: 'http://example.test/a'}
    timeout_ms: 1000
  - name: b
    kind: chrome
    host: other
    display: {host: other}
    prepare: {kind: launch, url: 'http://example.test/b'}
    timeout_ms: 1000
flows:
  - path: flow.yaml
    profiles: profiles.yaml
    bindings:
      - {target: a, profile: first}
      - {target: b, profile: second}
"#,
            )
            .unwrap();
            fs::write(root.join("flow.yaml"), flow).unwrap();
            fs::write(root.join("profiles.yaml"), profiles).unwrap();
            Self { root }
        }
        fn load(&self) -> Project {
            Project::load(&self.root).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[derive(Clone, Default)]
    struct Factory {
        events: Arc<StdMutex<Vec<String>>>,
        missing_tool: bool,
        missing_web_proxy: bool,
        fail_boot: bool,
        hang_tap: bool,
        fail_close: bool,
        capability_status: Option<String>,
        host_os: Option<String>,
        discovery_a: Discovery,
    }
    #[derive(Clone, Copy, Default)]
    enum Discovery {
        #[default]
        Ready,
        Delayed,
        Missing,
        StalledAfterEmpty,
        Ambiguous,
        ToolFailure,
    }
    struct Fake {
        name: String,
        factory: Factory,
        discoveries: usize,
    }
    #[async_trait]
    impl SessionFactory for Factory {
        async fn connect(
            &self,
            _: &Project,
            _: &Host,
            resource: &Resource,
        ) -> mcp::Result<Box<dyn Mcp>> {
            self.events
                .lock()
                .unwrap()
                .push(format!("connect:{}", resource.name));
            Ok(Box::new(Fake {
                name: resource.name.clone(),
                factory: self.clone(),
                discoveries: 0,
            }))
        }
    }
    #[async_trait]
    impl Mcp for Fake {
        async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
            match method {
                "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
                "tools/list" => {
                    let mut names = vec![
                        "mobile_list_available_devices",
                        "mobile_boot_device",
                        "mobile_open_url",
                        "mobile_get_capabilities",
                    ];
                    if !self.factory.missing_tool {
                        names.push("mobile_click_on_screen_at_coordinates");
                        names.push("mobile_type_keys");
                    }
                    if !self.factory.missing_web_proxy {
                        names.push(mcp::WEB_TEST_PROXY);
                    }
                    Ok(
                        json!({"tools": names.iter().map(|name| json!({"name":name,"inputSchema":{"type":"object"}})).collect::<Vec<_>>() }),
                    )
                }
                "tools/call" => {
                    let name = params["name"].as_str().unwrap();
                    let id = params["arguments"]["deviceId"].as_str().unwrap_or("");
                    self.factory
                        .events
                        .lock()
                        .unwrap()
                        .push(format!("{name}:{}:{id}", self.name));
                    if name == "mobile_click_on_screen_at_coordinates" && self.factory.hang_tap {
                        std::future::pending::<()>().await;
                    }
                    if name == "mobile_boot_device" && self.factory.fail_boot {
                        return Err("boot failed".into());
                    }
                    if name == mcp::WEB_TEST_PROXY {
                        assert_eq!(params["arguments"]["action"], "configure");
                        assert_eq!(params["arguments"]["config"]["upstream"], "127.0.0.1:8080");
                        return Ok(json!({"structuredContent":{"configured":true}}));
                    }
                    if name == "mobile_get_capabilities" {
                        return Ok(
                            json!({"structuredContent":{"hostOs":self.factory.host_os.as_deref().unwrap_or("macos"),"deviceId":id,"capabilities":{"discovery":{"status":"available"},"input":{"status":self.factory.capability_status.as_deref().unwrap_or("available")}}}}),
                        );
                    }
                    if name == "mobile_list_available_devices" {
                        self.discoveries += 1;
                        let mut devices = vec![
                            json!({"id":"chrome:same","name":"page","platform":"chrome","state":"available"}),
                        ];
                        if self.name == "a" {
                            match self.factory.discovery_a {
                                Discovery::Missing => devices.clear(),
                                Discovery::Delayed if self.discoveries <= 2 => devices.clear(),
                                Discovery::StalledAfterEmpty if self.discoveries == 1 => devices.clear(),
                                Discovery::StalledAfterEmpty => return std::future::pending().await,
                                Discovery::Ambiguous => devices.push(
                                    json!({"id":"chrome:other","name":"other page","platform":"chrome","state":"available"}),
                                ),
                                Discovery::ToolFailure => return Ok(json!({"isError":true,"content":[{"text":"private-discovery-detail"}]})),
                                _ => {}
                            }
                        }
                        return Ok(json!({"structuredContent":{"devices":devices}}));
                    }
                    Ok(json!({"content":[]}))
                }
                _ => Err("unexpected method".into()),
            }
        }
        async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
            Ok(())
        }
        async fn close(&mut self) -> mcp::Result<()> {
            self.factory
                .events
                .lock()
                .unwrap()
                .push(format!("close:{}", self.name));
            if self.factory.fail_close {
                return Err("disconnected during cleanup".into());
            }
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RequestConcurrency {
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        calls: Arc<StdMutex<Vec<(String, String)>>>,
    }

    struct ConcurrentFake {
        resource: String,
        state: RequestConcurrency,
    }

    #[async_trait]
    impl Mcp for ConcurrentFake {
        async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
            if method != "tools/call" {
                return Err("unexpected method".into());
            }
            let device_id = params["arguments"]["deviceId"]
                .as_str()
                .ok_or("device ID missing")?
                .to_owned();
            let active = self.state.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.state.maximum.fetch_max(active, Ordering::SeqCst);
            self.state
                .calls
                .lock()
                .unwrap()
                .push((self.resource.clone(), device_id));
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.state.active.fetch_sub(1, Ordering::SeqCst);
            Ok(json!({"content":[]}))
        }

        async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
            Ok(())
        }

        async fn close(&mut self) -> mcp::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn router_serializes_input_calls_across_resource_sessions() {
        let state = RequestConcurrency::default();
        let sessions: Shared = Arc::new(Mutex::new(BTreeMap::from([
            (
                "project:a".into(),
                Connected {
                    client: Box::new(ConcurrentFake {
                        resource: "a".into(),
                        state: state.clone(),
                    }),
                    real_id: "native-a".into(),
                    device: json!({"id":"project:a"}),
                },
            ),
            (
                "project:b".into(),
                Connected {
                    client: Box::new(ConcurrentFake {
                        resource: "b".into(),
                        state: state.clone(),
                    }),
                    real_id: "native-b".into(),
                    device: json!({"id":"project:b"}),
                },
            ),
        ])));
        let mut router_a = Router {
            sessions: Arc::clone(&sessions),
            devices: BTreeMap::new(),
            tools: BTreeMap::new(),
        };
        let mut router_b = Router {
            sessions,
            devices: BTreeMap::new(),
            tools: BTreeMap::new(),
        };

        let (result_a, result_b) = tokio::join!(
            router_a.request(
                "tools/call",
                json!({"name":"mobile_click_on_screen_at_coordinates","arguments":{"deviceId":"project:a","x":1,"y":1}}),
            ),
            router_b.request(
                "tools/call",
                json!({"name":"mobile_click_on_screen_at_coordinates","arguments":{"deviceId":"project:b","x":2,"y":2}}),
            ),
        );

        assert!(result_a.is_ok());
        assert!(result_b.is_ok());
        assert_eq!(state.maximum.load(Ordering::SeqCst), 1);
        let mut calls = state.calls.lock().unwrap().clone();
        calls.sort();
        assert_eq!(
            calls,
            vec![
                ("a".to_owned(), "native-a".to_owned()),
                ("b".to_owned(), "native-b".to_owned()),
            ]
        );
    }

    const PROFILES: &str = "version: 1\ntargets:\n  first: {}\n  second: {}\n";

    #[tokio::test]
    async fn proxy_configures_only_its_owned_resource_before_boot() {
        let fixture = Fixture::new(
            "version: 2\nname: wait\nsteps:\n  - wait: {ms: 0}\n",
            PROFILES,
        );
        let path = fixture.root.join("project.yaml");
        let manifest = fs::read_to_string(&path).unwrap();
        let configured = manifest.replace(
            "    prepare: {kind: launch, url: 'http://example.test/a'}",
            "    web_proxy: {upstream: '${env:PROXY_SPIKE_ADDR}'}\n    prepare: {kind: launch, url: 'http://example.test/a'}",
        );
        let configured = configured.replace("kind: project\n", "kind: project\nenv_file: .env\n");
        fs::write(&path, &configured).unwrap();
        fs::write(
            fixture.root.join(".env"),
            "PROXY_SPIKE_ADDR=127.0.0.1:8080\n",
        )
        .unwrap();
        let project = fixture.load();

        for missing_web_proxy in [false, true] {
            let factory = Factory {
                missing_web_proxy,
                ..Factory::default()
            };
            let outcome = run(
                &project,
                &factory,
                &fixture.root.join(format!("run-{missing_web_proxy}")),
                &[],
                Duration::from_secs(3),
            )
            .await;
            let events = factory.events.lock().unwrap();
            let configure = events
                .iter()
                .position(|e| e == "mobile_web_test_proxy:a:chrome:browser");
            let boot = events
                .iter()
                .position(|e| e == "mobile_boot_device:a:chrome:browser");
            if missing_web_proxy {
                assert!(
                    outcome
                        .err()
                        .expect("missing proxy tool must fail")
                        .to_string()
                        .contains("requires unavailable tool mobile_web_test_proxy")
                );
                assert!(configure.is_none() && boot.is_none());
            } else {
                assert!(outcome.is_ok(), "{:?}", outcome.err());
                assert!(configure.unwrap() < boot.unwrap(), "{events:?}");
                assert!(
                    !events
                        .iter()
                        .any(|e| e.starts_with("mobile_web_test_proxy:b:"))
                );
            }
        }

        fs::write(
            &path,
            configured.replace(
                "prepare: {kind: launch, url: 'http://example.test/a'}",
                "prepare: {kind: attach}",
            ),
        )
        .unwrap();
        assert!(
            Project::load(&fixture.root)
                .unwrap_err()
                .to_string()
                .contains("project-owned launched Chrome")
        );
    }

    #[tokio::test]
    async fn launched_chrome_waits_for_discovery_without_replaying_the_launch() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            discovery_a: Discovery::Delayed,
            ..Default::default()
        };
        let output = fixture.root.join("out");
        let result = run(
            &fixture.load(),
            &factory,
            &output,
            &[],
            Duration::from_secs(5),
        )
        .await
        .expect("the project should wait until the launched page is discoverable");
        assert_eq!(result.reports.len(), 2);
        assert!(result.reports.iter().all(|report| report.status == "PASS"));
        let events = factory.events.lock().unwrap();
        let browser_a: Vec<_> = events
            .iter()
            .filter(|event| event.split(':').nth(1) == Some("a"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            browser_a,
            [
                "connect:a",
                "mobile_boot_device:a:chrome:browser",
                "mobile_list_available_devices:a:",
                "mobile_list_available_devices:a:",
                "mobile_list_available_devices:a:",
                "mobile_get_capabilities:a:chrome:same",
                "mobile_open_url:a:chrome:same",
                "mobile_click_on_screen_at_coordinates:a:chrome:same",
                "close:a",
            ]
        );
    }

    #[tokio::test]
    async fn unready_or_ambiguous_chrome_never_navigates_and_always_closes_sessions() {
        for (discovery, attach, expected_kind, expected_message, expected_attempts) in [
            (
                Discovery::Missing,
                false,
                mcp::ErrorKind::Timeout,
                "resource a discovery timed out",
                None,
            ),
            (
                Discovery::StalledAfterEmpty,
                false,
                mcp::ErrorKind::Timeout,
                "resource a discovery timed out",
                Some(2),
            ),
            (
                Discovery::Missing,
                true,
                mcp::ErrorKind::Setup,
                "resource a matched 0 available devices",
                Some(1),
            ),
            (
                Discovery::Ambiguous,
                false,
                mcp::ErrorKind::Setup,
                "resource a matched 2 available devices",
                Some(1),
            ),
            (
                Discovery::ToolFailure,
                false,
                mcp::ErrorKind::Execution,
                "resource a discovery request failed",
                Some(1),
            ),
        ] {
            let fixture = Fixture::new(
                "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
                PROFILES,
            );
            let mut project = fixture.load();
            project.plan.resources[0].timeout_ms =
                if matches!(discovery, Discovery::StalledAfterEmpty) {
                    1000
                } else {
                    250
                };
            if attach {
                project.plan.resources[0].prepare = Preparation::Attach {};
            }
            let factory = Factory {
                discovery_a: discovery,
                ..Default::default()
            };
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                run(
                    &project,
                    &factory,
                    &fixture.root.join("out"),
                    &[],
                    Duration::from_secs(5),
                ),
            )
            .await
            .expect("resource discovery must be bounded");
            let Err(error) = result else {
                panic!("discovery must fail")
            };
            assert_eq!(error.kind(), expected_kind, "{error}");
            assert!(error.to_string().contains(expected_message), "{error}");
            assert!(!error.to_string().contains("private-discovery-detail"));
            let events = factory.events.lock().unwrap();
            let count = |event: &str| events.iter().filter(|value| *value == event).count();
            if let Some(expected_attempts) = expected_attempts {
                assert_eq!(count("mobile_list_available_devices:a:"), expected_attempts);
            }
            assert_eq!(
                count("mobile_boot_device:a:chrome:browser"),
                usize::from(!attach)
            );
            assert!(
                !events
                    .iter()
                    .any(|event| event.starts_with("mobile_open_url")
                        || event.starts_with("mobile_click"))
            );
            assert_eq!(count("close:a"), 1);
            assert_eq!(count("close:b"), 1);
        }
    }

    #[tokio::test]
    async fn prepares_all_owned_sessions_then_routes_each_case_to_its_own_native_id() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - select_device: '${device}'\n  - tap: {x: 1, y: 2}\n",
            "version: 1\ntargets:\n  first: {device: {id: native-first}}\n  second: {device: {id: native-second}}\n",
        );
        let factory = Factory::default();
        let result = run(
            &fixture.load(),
            &factory,
            &fixture.root.join("out"),
            &[],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(result.reports.len(), 2);
        assert!(result.reports.iter().all(|report| report.status == "PASS"));
        let events = factory.events.lock().unwrap();
        let first_tap = events
            .iter()
            .position(|v| v.starts_with("mobile_click"))
            .unwrap();
        assert!(
            events[..first_tap]
                .iter()
                .any(|v| v == "mobile_open_url:a:chrome:same")
        );
        assert!(
            events[..first_tap]
                .iter()
                .any(|v| v == "mobile_open_url:b:chrome:same")
        );
        assert!(
            events
                .iter()
                .any(|v| v == "mobile_click_on_screen_at_coordinates:a:chrome:same")
        );
        assert!(
            events
                .iter()
                .any(|v| v == "mobile_click_on_screen_at_coordinates:b:chrome:same")
        );
        assert!(events.iter().any(|v| v == "close:a"));
        assert!(events.iter().any(|v| v == "close:b"));
        let last_case_action = events
            .iter()
            .rposition(|v| v.starts_with("mobile_click_on_screen_at_coordinates:"))
            .unwrap();
        let first_close = events.iter().position(|v| v.starts_with("close:")).unwrap();
        assert!(
            first_close > last_case_action,
            "project-owned sessions must stay open through every target case"
        );
        assert_eq!(events.iter().filter(|v| *v == "close:a").count(), 1);
        assert_eq!(events.iter().filter(|v| *v == "close:b").count(), 1);
    }
    #[tokio::test]
    async fn missing_tool_and_profile_fail_before_launch_and_close_owned_connections() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            missing_tool: true,
            ..Factory::default()
        };
        assert!(
            run(
                &fixture.load(),
                &factory,
                &fixture.root.join("out"),
                &[],
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
        {
            let events = factory.events.lock().unwrap();
            assert!(!events.iter().any(|event| event.starts_with("mobile_boot")));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.starts_with("close:"))
                    .count(),
                2
            );
        }
        fs::write(
            fixture.root.join("profiles.yaml"),
            "version: 1\ntargets: {first: {}}\n",
        )
        .unwrap();
        let factory = Factory::default();
        assert!(
            run(
                &fixture.load(),
                &factory,
                &fixture.root.join("out"),
                &[],
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
        assert!(factory.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn explicit_project_cancellation_closes_owned_sessions() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            hang_tap: true,
            ..Factory::default()
        };
        let owned_factory = factory.clone();
        let project = fixture.load();
        let output = fixture.root.join("cancel-out");
        let (cancel, receiver) = watch::channel(false);
        let task = tokio::spawn(async move {
            run_with_history_cancel(
                &project,
                &owned_factory,
                &output,
                &[],
                Duration::from_secs(10),
                None,
                receiver,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if factory
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|event| event.starts_with("mobile_click"))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        cancel.send(true).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        let error = match result {
            Ok(_) => panic!("project cancellation should interrupt the run"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), robot_flow::outcome::ErrorKind::Interrupted);
        assert_eq!(
            factory
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.starts_with("close:"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn setup_failure_closes_every_owned_session() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            fail_boot: true,
            ..Factory::default()
        };
        assert!(
            run(
                &fixture.load(),
                &factory,
                &fixture.root.join("out"),
                &[],
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
        let events = factory.events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("close:"))
                .count(),
            2
        );
        assert!(!events.iter().any(|event| event.starts_with("mobile_click")));
    }
    #[tokio::test]
    async fn deadline_and_dropped_project_future_close_owned_sessions() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            hang_tap: true,
            ..Factory::default()
        };
        let result = run(
            &fixture.load(),
            &factory,
            &fixture.root.join("out"),
            &[],
            Duration::from_millis(30),
        )
        .await
        .unwrap();
        assert_eq!(result.reports[0].error_kind.as_deref(), Some("timeout"));
        assert_eq!(
            factory
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.starts_with("close:"))
                .count(),
            2
        );
        let factory = Factory {
            hang_tap: true,
            ..Factory::default()
        };
        let owned_factory = factory.clone();
        let project = fixture.load();
        let output = fixture.root.join("other-out");
        let task = tokio::spawn(async move {
            run(
                &project,
                &owned_factory,
                &output,
                &[],
                Duration::from_secs(10),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if factory
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|event| event.starts_with("mobile_click"))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if factory
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|event| event.starts_with("close:"))
                    .count()
                    == 2
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dotenv_input_is_redacted_without_scrubbing_unreferenced_values() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - id: step-1\n    type_text: {text: '${env:DEVICE_PROJECT_TEST_SECRET_482}'}\n",
            PROFILES,
        );
        let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
            .unwrap()
            .replacen("name: test", "name: test\nenv_file: .env", 1);
        fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
        fs::write(
            fixture.root.join(".env"),
            "DEVICE_PROJECT_TEST_SECRET_482=private-value-482\nDEVICE_PROJECT_TEST_UNUSED_482=1\n",
        )
        .unwrap();
        let factory = Factory::default();
        let output = fixture.root.join("out");
        let result = run(
            &fixture.load(),
            &factory,
            &output,
            &[],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(result.reports.len(), 2);
        let evidence = fs::read_to_string(output.join("case-001/expanded-flow.yaml")).unwrap();
        assert!(!evidence.contains("private-value-482"));
        assert!(evidence.contains("step-1"));
    }
    #[tokio::test]
    async fn completed_case_evidence_survives_cleanup_failure() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let factory = Factory {
            fail_close: true,
            ..Factory::default()
        };
        let output = fixture.root.join("out");
        assert!(
            run(
                &fixture.load(),
                &factory,
                &output,
                &[],
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
        assert!(output.join("case-001/report.json").is_file());
        assert!(output.join("case-001/junit.xml").is_file());
        assert!(output.join("case-002/report.json").is_file());
    }

    #[tokio::test]
    async fn compile_error_redacts_referenced_environment_values() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - platform: '${env:DEVICE_PROJECT_TEST_BAD_PLATFORM_482}'\n    tap: {x: 1, y: 2}\n",
            PROFILES,
        );
        let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
            .unwrap()
            .replacen("name: test", "name: test\nenv_file: .env", 1);
        fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
        fs::write(
            fixture.root.join(".env"),
            "DEVICE_PROJECT_TEST_BAD_PLATFORM_482=private-invalid-platform-482\n",
        )
        .unwrap();
        let error = match run(
            &fixture.load(),
            &Factory::default(),
            &fixture.root.join("out"),
            &[],
            Duration::from_secs(5),
        )
        .await
        {
            Ok(_) => panic!("expected compile failure"),
            Err(error) => error.to_string(),
        };
        assert!(!error.contains("private-invalid-platform-482"));
        assert!(error.contains("<REDACTED>"));
    }
    #[tokio::test]
    async fn capability_and_host_failures_stop_before_url_navigation() {
        for (status, host, expected_error) in [
            (
                Some("blocked"),
                None,
                "resource a requires input, capability status blocked",
            ),
            (
                Some("unsupported"),
                None,
                "resource a requires input, capability status unsupported",
            ),
            (
                Some("unknown"),
                None,
                "resource a requires input, capability status unknown",
            ),
            (
                None,
                Some("windows"),
                "resource a host OS does not match declared host",
            ),
        ] {
            let fixture = Fixture::new(
                "version: 2\nname: test\nsteps:\n  - tap: {x: 1, y: 2}\n",
                PROFILES,
            );
            let manifest = fs::read_to_string(fixture.root.join("project.yaml"))
                .unwrap()
                .replacen(
                    "    timeout_ms: 1000",
                    "    requires: [input]\n    timeout_ms: 1000",
                    1,
                );
            fs::write(fixture.root.join("project.yaml"), manifest).unwrap();
            let factory = Factory {
                capability_status: status.map(str::to_owned),
                host_os: host.map(str::to_owned),
                ..Factory::default()
            };
            let result = run(
                &fixture.load(),
                &factory,
                &fixture.root.join("out"),
                &[],
                Duration::from_secs(5),
            )
            .await;
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("unsupported or mismatched targets must fail setup"),
            };
            assert!(error.to_string().contains(expected_error), "{error}");
            let events = factory.events.lock().unwrap();
            assert!(
                !events
                    .iter()
                    .any(|v| v.starts_with("mobile_open_url") || v.starts_with("mobile_click"))
            );
            assert_eq!(events.iter().filter(|v| v.starts_with("close:")).count(), 2);
        }
    }

    #[tokio::test]
    async fn literal_input_is_not_copied_into_evidence_or_history() {
        let fixture = Fixture::new(
            "version: 2\nname: test\nsteps:\n  - type_text: {text: literal-private-value-482}\n",
            PROFILES,
        );
        let factory = Factory::default();
        let output = fixture.root.join("out");
        let history = fixture.root.join("history");
        run_with_history(
            &fixture.load(),
            &factory,
            &output,
            &[],
            Duration::from_secs(5),
            Some(&history),
        )
        .await
        .unwrap();
        fn inspect(path: &Path) {
            if path.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    inspect(&entry.unwrap().path());
                }
            } else if let Ok(value) = fs::read_to_string(path) {
                assert!(
                    !value.contains("literal-private-value-482"),
                    "secret in {}",
                    path.display()
                );
            }
        }
        inspect(&output);
        inspect(&history);
    }
}

//! Project-lifetime MCP sessions and named device routing.
use crate::{
    Project,
    plan::{Capability, Host, HostBinding, HostPlatform, Preparation, Resource, ResourceKind},
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
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

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
struct CleanupGuard(Option<Shared>);
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let (Some(sessions), Ok(handle)) = (self.0.take(), tokio::runtime::Handle::try_current())
        {
            handle.spawn(close_owned(sessions));
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
            let resource = project
                .plan
                .resources
                .iter()
                .find(|resource| resource.name == binding.target)
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
                target: binding.target.clone(),
            });
        }
    }
    Ok(cases)
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
    let mut cleanup = CleanupGuard(Some(Arc::clone(&sessions)));
    let context = RunContext {
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
    };
    let result = tokio::select! {
        result = run_owned(context, environment, cases, Arc::clone(&sessions)) => result,
        _ = interrupted() => Err("project interrupted".into()),
    };
    let clean = close_owned(Arc::clone(&sessions)).await;
    cleanup.0 = None;
    match (result, clean) {
        (Err(error), _) => Err(error),
        (Ok(_), false) => Err("project MCP cleanup failed".into()),
        (Ok(result), true) => Ok(result),
    }
}

async fn interrupted() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
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
    let result = tokio::time::timeout(
        limit,
        client.request("tools/call", json!({"name":name,"arguments":arguments})),
    )
    .await
    .map_err(|_| format!("project tool {name} timed out"))??;
    if result["isError"] == true {
        return Err(format!("project tool {name} failed").into());
    }
    Ok(result)
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
    for case in &cases {
        let mut runner = FlowRunner::new(None, directory.to_owned());
        runner.set_environment(environment.clone());
        runner.set_named_resources(devices.clone());
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
                invoke(
                    connection.client.as_mut(),
                    "mobile_boot_device",
                    json!({"deviceId":"chrome:browser"}),
                    limit,
                )
                .await?;
            }
            let result = invoke(
                connection.client.as_mut(),
                "mobile_list_available_devices",
                json!({}),
                limit,
            )
            .await?;
            let available = result
                .pointer("/structuredContent/devices")
                .and_then(Value::as_array)
                .ok_or("project discovery returned no devices")?;
            let selector = resource.selector.as_deref();
            let matched: Vec<_> = available
                .iter()
                .filter(|d| {
                    d["platform"] == platform(resource.kind)
                        && selector.is_none_or(|s| {
                            d["id"].as_str() == Some(s) || d["name"].as_str() == Some(s)
                        })
                        && d["state"] == "available"
                })
                .collect();
            if matched.len() != 1 {
                return Err(format!(
                    "resource {} matched {} available devices; require exactly one",
                    resource.name,
                    matched.len()
                )
                .into());
            }
            let mut device = matched[0].clone();
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
    let mut reports = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let default = devices
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
        runner.set_named_resources(devices.clone());
        runner.set_default_device(default);
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
    use std::{fs, sync::Mutex as StdMutex};

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
        fail_boot: bool,
        hang_tap: bool,
        fail_close: bool,
        capability_status: Option<String>,
        host_os: Option<String>,
    }
    struct Fake {
        name: String,
        factory: Factory,
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
                    if name == "mobile_get_capabilities" {
                        return Ok(
                            json!({"structuredContent":{"hostOs":self.factory.host_os.as_deref().unwrap_or("macos"),"deviceId":id,"capabilities":{"discovery":{"status":"available"},"input":{"status":self.factory.capability_status.as_deref().unwrap_or("available")}}}}),
                        );
                    }
                    if name == "mobile_list_available_devices" {
                        return Ok(
                            json!({"structuredContent":{"devices":[{"id":"chrome:same","name":"page","platform":"chrome","state":"available"}]}}),
                        );
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
    const PROFILES: &str = "version: 1\ntargets:\n  first: {}\n  second: {}\n";
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
        for (status, host) in [
            (Some("blocked"), None),
            (Some("unknown"), None),
            (None, Some("windows")),
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

//! Flow lifecycle, cancellation, device scope, and MCP session ownership.
//!
//! Action dispatch, preflight, element queries, and evidence handling live in
//! private child modules. They share this runner's session state without adding
//! a second public interface for callers to coordinate.

mod actions;
mod arguments;
mod artifacts;
mod elements;
mod network_fault;
mod preflight;

use crate::outcome::{ErrorKind, FlowError, FlowStatus, StepStatus};
use crate::{
    mcp::{self, Mcp, Result},
    schema::{self, FlowDocument, Profile, Step},
};
use artifacts::collect_secrets;
pub use elements::filter_elements;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::time::{Instant, timeout};

const QUERY: &str = "mobile_list_elements_on_screen";
const TAP: &str = "mobile_click_on_screen_at_coordinates";
const SWIPE: &str = "mobile_swipe_on_screen";
const TYPE: &str = "mobile_type_keys";
const BATCH: &str = "mobile_execute_ui_actions";

#[derive(Debug, Serialize)]
pub struct StepReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub id: String,
    pub action: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_probe: Option<ElementProbeReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_fault: Option<NetworkFaultReport>,
}

pub use network_fault::{NetworkFaultReport, NetworkFaultSnapshot};

#[derive(Clone)]
enum StepEvidence {
    ElementProbe(ElementProbeReport),
    NetworkFault(NetworkFaultReport),
}

#[derive(Clone, Debug, Serialize)]
pub struct ElementProbeReport {
    pub first_match_ms: Option<u64>,
    pub samples: Vec<ElementProbeSample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ElementProbeSample {
    pub count: usize,
    pub query_ms: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct FlowReport {
    pub name: String,
    pub dry_run: bool,
    pub status: String,
    pub selected_device: Option<Value>,
    pub steps: Vec<StepReport>,
    pub limitations: Vec<String>,
    pub preconditions: Vec<String>,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

/// Boundary event for one flow step. This intentionally excludes action
/// arguments so callers can attach timing metadata without copying user input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowStepEvent {
    pub phase: FlowStepPhase,
    pub step_id: String,
    pub action: String,
    pub resource: Option<String>,
    pub platform: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowStepPhase {
    Started,
    Passed,
    Failed,
}

impl FlowStepPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

/// Synchronous observer invoked at step execution boundaries.
pub type FlowStepEventSink = Arc<dyn Fn(FlowStepEvent) + Send + Sync>;

pub struct FlowRunner {
    client: Option<Box<dyn Mcp>>,
    tools: BTreeMap<String, Value>,
    profile: Profile,
    device: Option<Value>,
    step_device: Option<Value>,
    named_resources: BTreeMap<String, Value>,
    environment: Option<BTreeMap<String, String>>,
    recording_device: Option<String>,
    directory: PathBuf,
    values: BTreeMap<String, String>,
    focused_web_contexts: BTreeMap<String, Value>,
    secrets: Vec<String>,
    recording: bool,
    query_count: usize,
    steps: Vec<StepReport>,
    limitations: Vec<String>,
    version: u32,
    active_step: Option<(String, String)>,
    active_note: Option<String>,
    video_overlay: Option<crate::video::VideoOverlay>,
    execution_snapshot: Option<Value>,
    step_event_sink: Option<FlowStepEventSink>,
    owned_faults: BTreeSet<String>,
    run_started: Instant,
}

impl FlowRunner {
    pub fn new(client: Option<Box<dyn Mcp>>, directory: PathBuf) -> Self {
        Self {
            client,
            tools: BTreeMap::new(),
            profile: Profile::default(),
            device: None,
            step_device: None,
            named_resources: BTreeMap::new(),
            environment: None,
            recording_device: None,
            directory,
            values: BTreeMap::new(),
            focused_web_contexts: BTreeMap::new(),
            secrets: Vec::new(),
            recording: false,
            query_count: 0,
            steps: Vec::new(),
            limitations: Vec::new(),
            version: 2,
            active_step: None,
            active_note: None,
            video_overlay: None,
            execution_snapshot: None,
            step_event_sink: None,
            owned_faults: BTreeSet::new(),
            run_started: Instant::now(),
        }
    }

    /// Observe step boundaries without receiving input values or MCP payloads.
    pub fn set_step_event_sink(&mut self, sink: FlowStepEventSink) {
        self.step_event_sink = Some(sink);
    }

    /// Set stable resource names to normalized device objects for scoped steps.
    pub fn set_named_resources(&mut self, resources: BTreeMap<String, Value>) {
        self.named_resources = resources;
    }

    /// Use a scoped environment for all flow compilation.
    pub fn set_environment(&mut self, environment: BTreeMap<String, String>) {
        self.environment = Some(environment);
    }

    /// Validate a case against advertised tools and named devices without making
    /// any MCP calls. Run still performs its own validation before execution.
    pub fn preflight_case(
        &mut self,
        flow: &FlowDocument,
        profile: &Profile,
        tools: BTreeMap<String, Value>,
    ) -> Result<()> {
        let (steps, resolved_profile) =
            schema::compile_with_environment(flow, profile, self.environment.as_ref())?;
        self.profile = resolved_profile;
        self.version = flow.version;
        self.tools = tools;
        // An empty advertised list still means tools must be validated.
        if self.tools.is_empty()
            && steps.iter().any(|step| {
                !step.action.contains_key("wait")
                    && !step.action.contains_key("note_limitation")
                    && !step.action.contains_key("reset_fields")
            })
        {
            return Err("required MCP tools unavailable".into());
        }
        self.preflight(&steps)
    }

    /// Set the default project target without issuing an MCP discovery call.
    pub fn set_default_device(&mut self, device: Value) {
        self.device = Some(device);
    }

    pub async fn run(
        &mut self,
        flow: &FlowDocument,
        profile: &Profile,
        confirmed: &[String],
        case_timeout: Duration,
    ) -> FlowReport {
        self.run_case(flow, profile, confirmed, Some(case_timeout))
            .await
    }

    /// Run an embedded case without a second wall-clock deadline. Individual
    /// steps and driver operations retain their own timeout and error reports.
    pub async fn run_without_case_timeout(
        &mut self,
        flow: &FlowDocument,
        profile: &Profile,
        confirmed: &[String],
    ) -> FlowReport {
        self.run_case(flow, profile, confirmed, None).await
    }

    async fn run_case(
        &mut self,
        flow: &FlowDocument,
        profile: &Profile,
        confirmed: &[String],
        case_timeout: Option<Duration>,
    ) -> FlowReport {
        self.profile = profile.clone();
        self.run_started = Instant::now();
        self.version = flow.version;
        let dry_run = self.client.is_none();
        if let Some(environment) = &self.environment {
            collect_environment_secrets(
                &serde_json::to_value(flow).unwrap_or_default(),
                environment,
                &mut self.secrets,
            );
            collect_environment_secrets(
                &serde_json::to_value(profile).unwrap_or_default(),
                environment,
                &mut self.secrets,
            );
        }
        let preconditions = profile
            .preconditions
            .iter()
            .chain(&flow.preconditions)
            .cloned()
            .collect::<Vec<_>>();
        let result = async {
            let (steps, resolved_profile) =
                schema::compile_with_environment(flow, profile, self.environment.as_ref())?;
            self.profile = resolved_profile;
            collect_secrets(&serde_json::to_value(&steps).unwrap(), &mut self.secrets);
            self.execution_snapshot = Some(json!({
                "profile": self.profile,
                "flow": FlowDocument { steps: steps.clone(), ..flow.clone() },
            }));
            if !dry_run {
                for condition in &preconditions {
                    if !confirmed.contains(condition) {
                        return Err(format!(
                            "setup precondition not confirmed: {condition} (use --confirm after completing it outside the runner)"
                        ).into());
                    }
                }
                fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
                self.save_execution_snapshot()?;
                self.client = Some(Box::new(crate::trace::TracedMcp::new(
                    self.client.take().unwrap(),
                    self.directory.join("mcp-calls.jsonl"),
                )));
                self.tools = mcp::initialize(self.client.as_mut().unwrap().as_mut()).await?;
            }
            self.preflight(&steps)?;
            self.execute_steps(&steps).await
        };
        let case = async {
            match case_timeout {
                Some(limit) => timeout(limit, result).await,
                None => Ok(result.await),
            }
        };
        let outcome = tokio::select! {result=case=>result,_=interrupted()=>Ok(Err(FlowError::new(ErrorKind::Interrupted, "runner interrupted")))};
        // A case deadline can drop nested step futures before their normal restoration.
        self.step_device = None;
        let mut error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(if self.steps.is_empty() {
                e.during_setup()
            } else {
                e
            }),
            Err(_) => Some(FlowError::new(
                ErrorKind::Timeout,
                self.active_step
                    .as_ref()
                    .map(|(id, action)| format!("scenario timeout at step {id} ({action})"))
                    .unwrap_or("scenario timeout during setup".into()),
            )),
        };
        // A case deadline or signal drops the active execute_steps future before
        // it can append its StepReport. Preserve that failed leaf in the report.
        if let (Some(error), Some((id, action))) = (&error, &self.active_step) {
            if !self
                .steps
                .iter()
                .any(|s| s.id == *id && s.status == StepStatus::Failed.as_str())
            {
                self.steps.push(StepReport {
                    note: self.active_note.as_ref().map(|note| self.redact(note)),
                    id: id.clone(),
                    action: action.clone(),
                    status: StepStatus::Failed.as_str().into(),
                    error: Some(self.redact(&error.to_string())),
                    element_probe: None,
                    network_fault: None,
                });
            }
        }
        if self.recording {
            if let Err(e) = self.stop_recording().await {
                append_error(&mut error, e.context("recording cleanup"));
            }
        }
        if !self.owned_faults.is_empty() {
            let cleanup = timeout(Duration::from_secs(5), self.clear_owned_faults())
                .await
                .unwrap_or_else(|_| {
                    Err(FlowError::new(
                        ErrorKind::Timeout,
                        "web fault cleanup timed out",
                    ))
                });
            if let Err(failure) = cleanup {
                append_error(&mut error, failure.context("web fault cleanup"));
            }
        }
        if let Some(client) = self.client.as_mut() {
            if let Err(e) = client.close().await {
                append_error(&mut error, e.into_transport().context("MCP cleanup"));
            }
        }
        let error_kind = error.as_ref().map(|e| e.kind().as_str().to_owned());
        FlowReport {
            name: flow.name.clone(),
            dry_run,
            status: if error.is_some() {
                FlowStatus::Fail
            } else if dry_run {
                FlowStatus::DryRun
            } else if self.limitations.is_empty() {
                FlowStatus::Pass
            } else {
                FlowStatus::Partial
            }
            .as_str()
            .into(),
            selected_device: self.device.clone(),
            steps: std::mem::take(&mut self.steps),
            limitations: self.limitations.clone(),
            preconditions,
            error: error.map(|e| self.redact(&e.to_string())),
            error_kind,
        }
    }

    fn execute_steps<'a>(
        &'a mut self,
        steps: &'a [Step],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let parent = self.active_step.clone();
            let parent_note = self.active_note.clone();
            for step in steps {
                let (action, args) = step.action.iter().next().unwrap();
                self.active_step = Some((step.id.clone().unwrap_or_default(), action.clone()));
                self.active_note = step.note.clone();
                self.emit_step_event(step, action, FlowStepPhase::Started);
                if self.recording && action != "stop_screen_recording" {
                    if let Some(overlay) = self.video_overlay.as_mut() {
                        overlay.mark(step.id.as_deref().unwrap_or(""), action, args);
                    }
                }
                let previous_device = self.step_device.clone();
                let result = timeout(
                    Duration::from_millis(
                        args.get("timeout_ms")
                            .and_then(Value::as_u64)
                            .unwrap_or(120_000),
                    ),
                    async {
                        if let Some(resource) = &step.resource {
                            self.step_device = Some(self.named_resources[resource].clone());
                        } else if let Some(platform) = &step.platform {
                            if self.client.is_some() {
                                self.step_device = Some(self.device_for_platform(platform).await?);
                            }
                        }
                        self.execute(action, args).await
                    },
                )
                .await
                .unwrap_or_else(|_| Err(FlowError::new(ErrorKind::Timeout, "step timeout")));
                self.step_device = previous_device;
                self.steps.push(StepReport {
                    note: step.note.as_ref().map(|note| self.redact(note)),
                    id: step.id.clone().unwrap_or_default(),
                    action: action.clone(),
                    status: if result.is_err() {
                        StepStatus::Failed
                    } else if self.client.is_none() {
                        StepStatus::Validated
                    } else {
                        StepStatus::Passed
                    }
                    .as_str()
                    .into(),
                    error: result.as_ref().err().map(|e| self.redact(&e.to_string())),
                    element_probe: match &result {
                        Ok(Some(StepEvidence::ElementProbe(report))) => Some(report.clone()),
                        _ => None,
                    },
                    network_fault: match &result {
                        Ok(Some(StepEvidence::NetworkFault(report))) => Some(report.clone()),
                        _ => None,
                    },
                });
                self.emit_step_event(
                    step,
                    action,
                    if result.is_err() {
                        FlowStepPhase::Failed
                    } else {
                        FlowStepPhase::Passed
                    },
                );
                if result.is_ok() {
                    self.active_step = parent.clone();
                    self.active_note = parent_note.clone();
                }
                result.map_err(|e| {
                    e.for_step(action).context(format!(
                        "step {} ({action})",
                        step.id.as_deref().unwrap_or("?")
                    ))
                })?;
            }
            Ok(())
        })
    }

    fn emit_step_event(&self, step: &Step, action: &str, phase: FlowStepPhase) {
        if let Some(sink) = &self.step_event_sink {
            sink(FlowStepEvent {
                phase,
                step_id: step.id.clone().unwrap_or_default(),
                action: action.to_owned(),
                resource: step.resource.clone(),
                platform: step.platform.clone(),
            });
        }
    }

    async fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        mcp::validate_arguments(
            self.tools
                .get(name)
                .ok_or_else(|| format!("missing MCP tool {name}"))?,
            &args,
        )
        .map_err(FlowError::into_transport)?;
        let result = self
            .client
            .as_mut()
            .ok_or("MCP unavailable in dry-run")?
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await
            .map_err(FlowError::into_transport)?;
        if result.get("isError") == Some(&json!(true)) {
            return Err(
                FlowError::from(format!("MCP tool {name} failed: {}", result["content"]))
                    .into_transport(),
            );
        }
        Ok(result)
    }

    async fn device_for_platform(&mut self, platform: &str) -> Result<Value> {
        // An explicitly selected tab/device disambiguates platforms with several targets.
        if let Some(device) = self.step_device.as_ref().or(self.device.as_ref()) {
            if device["platform"].as_str() == Some(platform) {
                return Ok(device.clone());
            }
        }
        if let Some(device) = &self.device {
            if device["platform"].as_str() == Some(platform) {
                return Ok(device.clone());
            }
        }
        let result = self
            .call("mobile_list_available_devices", json!({}))
            .await?;
        let devices = result
            .pointer("/structuredContent/devices")
            .and_then(Value::as_array)
            .ok_or("MCP devices missing")?;
        let matches = devices
            .iter()
            .filter(|d| d["platform"].as_str() == Some(platform))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "step platform {platform} matched {} devices; require exactly one (use select_device to disambiguate)",
                matches.len()
            ).into());
        }
        Ok(matches[0].clone())
    }

    fn device_id(&self) -> Result<&str> {
        self.step_device
            .as_ref()
            .or(self.device.as_ref())
            .and_then(|d| d["id"].as_str())
            .ok_or("select a device before UI actions".into())
    }
}

// Only referenced values are redacted; unrelated inherited values such as "1"
// must not corrupt step IDs, device names or diagnostics.
fn collect_environment_secrets(
    value: &Value,
    environment: &BTreeMap<String, String>,
    secrets: &mut Vec<String>,
) {
    fn add(name: &str, environment: &BTreeMap<String, String>, secrets: &mut Vec<String>) {
        if let Some(value) = environment.get(name).filter(|value| !value.is_empty()) {
            secrets.push(value.clone());
        }
    }
    match value {
        Value::String(text) => {
            let mut rest = text.as_str();
            while let Some(index) = rest.find("${env:") {
                if !rest[..index].ends_with('$') {
                    if let Some(end) = rest[index + 6..].find('}') {
                        add(&rest[index + 6..index + 6 + end], environment, secrets);
                    }
                }
                rest = &rest[index + 6..];
            }
        }
        Value::Object(map) => {
            if let Some(name) = map.get("text_from_env").and_then(Value::as_str) {
                add(name, environment, secrets);
            }
            for child in map.values() {
                collect_environment_secrets(child, environment, secrets);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_environment_secrets(child, environment, secrets);
            }
        }
        _ => {}
    }
}

/// Scrub referenced scoped environment values from setup diagnostics.
pub fn redact_environment_error(
    message: &str,
    flow: &FlowDocument,
    profile: &Profile,
    environment: &BTreeMap<String, String>,
) -> String {
    let mut secrets = Vec::new();
    collect_environment_secrets(
        &serde_json::to_value(flow).unwrap_or_default(),
        environment,
        &mut secrets,
    );
    collect_environment_secrets(
        &serde_json::to_value(profile).unwrap_or_default(),
        environment,
        &mut secrets,
    );
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut result = message.to_owned();
    for secret in secrets {
        result = result.replace(&secret, "<REDACTED>");
    }
    result
}

fn append_error(error: &mut Option<FlowError>, next: FlowError) {
    if let Some(primary) = error {
        primary.append(next);
    } else {
        *error = Some(next);
    }
}
async fn interrupted() {
    #[cfg(unix)]
    if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! {_=term.recv()=>{},_=ctrl_c_or_pending()=>{}}
        return;
    }
    ctrl_c_or_pending().await;
}

async fn ctrl_c_or_pending() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

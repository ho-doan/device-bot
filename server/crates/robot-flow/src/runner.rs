//! Flow lifecycle, cancellation, device scope, and MCP session ownership.
//!
//! Action dispatch, preflight, element queries, and evidence handling live in
//! private child modules. They share this runner's session state without adding
//! a second public interface for callers to coordinate.

mod actions;
mod arguments;
mod artifacts;
mod elements;
mod preflight;

use crate::{
    mcp::{self, Mcp, Result},
    schema::{self, FlowDocument, Profile, Step},
};
use artifacts::collect_secrets;
pub use elements::filter_elements;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, future::Future, path::PathBuf, pin::Pin, time::Duration};
use tokio::time::timeout;

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

pub struct FlowRunner {
    client: Option<Box<dyn Mcp>>,
    tools: BTreeMap<String, Value>,
    profile: Profile,
    device: Option<Value>,
    step_device: Option<Value>,
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
}

impl FlowRunner {
    pub fn new(client: Option<Box<dyn Mcp>>, directory: PathBuf) -> Self {
        Self {
            client,
            tools: BTreeMap::new(),
            profile: Profile::default(),
            device: None,
            step_device: None,
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
        }
    }

    pub async fn run(
        &mut self,
        flow: &FlowDocument,
        profile: &Profile,
        confirmed: &[String],
        case_timeout: Duration,
    ) -> FlowReport {
        self.profile = profile.clone();
        self.version = flow.version;
        let dry_run = self.client.is_none();
        let preconditions = profile
            .preconditions
            .iter()
            .chain(&flow.preconditions)
            .cloned()
            .collect::<Vec<_>>();
        let result = async {
            let (steps, resolved_profile) = schema::compile_with_profile(flow, profile)?;
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
                        ));
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
        let outcome = tokio::select! {result=timeout(case_timeout,result)=>result,_=interrupted()=>Ok(Err("runner interrupted".into()))};
        let mut error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(self.redact(&e)),
            Err(_) => Some(
                self.active_step
                    .as_ref()
                    .map(|(id, action)| format!("scenario timeout at step {id} ({action})"))
                    .unwrap_or("scenario timeout during setup".into()),
            ),
        };
        // A case deadline or signal drops the active execute_steps future before
        // it can append its StepReport. Preserve that failed leaf in the report.
        if let (Some(error), Some((id, action))) = (&error, &self.active_step) {
            if !self
                .steps
                .iter()
                .any(|s| s.id == *id && s.status == "failed")
            {
                self.steps.push(StepReport {
                    note: self.active_note.as_ref().map(|note| self.redact(note)),
                    id: id.clone(),
                    action: action.clone(),
                    status: "failed".into(),
                    error: Some(error.clone()),
                });
            }
        }
        if self.recording {
            if let Err(e) = self.stop_recording().await {
                append_error(
                    &mut error,
                    format!("recording cleanup: {}", self.redact(&e)),
                );
            }
        }
        if let Some(client) = self.client.as_mut() {
            if let Err(e) = client.close().await {
                append_error(&mut error, format!("MCP cleanup: {}", self.redact(&e)));
            }
        }
        let error_kind = error.as_ref().map(|e| {
            {
                if e.contains("timeout") {
                    "timeout"
                } else if e.contains("interrupted") {
                    "interrupted"
                } else if self.steps.is_empty() {
                    "setup"
                } else if e.contains("Unsupported") || e.contains("unknown, not false") {
                    "unsupported"
                } else if !e.contains("MCP")
                    && self
                        .steps
                        .iter()
                        .find(|s| s.status == "failed")
                        .is_some_and(|s| s.action.starts_with("assert_"))
                {
                    "assertion"
                } else {
                    "execution"
                }
            }
            .to_owned()
        });
        FlowReport {
            name: flow.name.clone(),
            dry_run,
            status: if error.is_some() {
                "FAIL"
            } else if dry_run {
                "DRY_RUN"
            } else if self.limitations.is_empty() {
                "PASS"
            } else {
                "PARTIAL"
            }
            .into(),
            selected_device: self.device.clone(),
            steps: std::mem::take(&mut self.steps),
            limitations: self.limitations.clone(),
            preconditions,
            error,
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
                        if let Some(platform) = &step.platform {
                            if self.client.is_some() {
                                self.step_device = Some(self.device_for_platform(platform).await?);
                            }
                        }
                        self.execute(action, args).await
                    },
                )
                .await
                .unwrap_or_else(|_| Err("step timeout".into()));
                self.step_device = previous_device;
                self.steps.push(StepReport {
                    note: step.note.as_ref().map(|note| self.redact(note)),
                    id: step.id.clone().unwrap_or_default(),
                    action: action.clone(),
                    status: if result.is_err() {
                        "failed"
                    } else if self.client.is_none() {
                        "validated"
                    } else {
                        "passed"
                    }
                    .into(),
                    error: result.as_ref().err().map(|e| self.redact(e)),
                });
                if result.is_ok() {
                    self.active_step = parent.clone();
                    self.active_note = parent_note.clone();
                }
                result.map_err(|e| {
                    format!("step {} ({action}): {e}", step.id.as_deref().unwrap_or("?"))
                })?;
            }
            Ok(())
        })
    }

    async fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        mcp::validate_arguments(
            self.tools
                .get(name)
                .ok_or_else(|| format!("missing MCP tool {name}"))?,
            &args,
        )?;
        let result = self
            .client
            .as_mut()
            .ok_or("MCP unavailable in dry-run")?
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await?;
        if result.get("isError") == Some(&json!(true)) {
            return Err(format!("MCP tool {name} failed: {}", result["content"]));
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
            ));
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

fn append_error(error: &mut Option<String>, message: String) {
    if let Some(e) = error {
        e.push_str("; ");
        e.push_str(&message);
    } else {
        *error = Some(message);
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

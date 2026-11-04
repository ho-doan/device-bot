use crate::{
    mcp::{self, Mcp, Result},
    schema::{self, FlowDocument, Profile, Selector, Step},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    path::{Component, Path, PathBuf},
    pin::Pin,
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout};

const QUERY: &str = "mobile_list_elements_on_screen";
const TAP: &str = "mobile_click_on_screen_at_coordinates";
const SWIPE: &str = "mobile_swipe_on_screen";
const TYPE: &str = "mobile_type_keys";
const BATCH: &str = "mobile_execute_ui_actions";

#[derive(Debug, Serialize)]
pub struct StepReport {
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
    directory: PathBuf,
    values: BTreeMap<String, String>,
    secrets: Vec<String>,
    recording: bool,
    query_count: usize,
    steps: Vec<StepReport>,
    limitations: Vec<String>,
    version: u32,
    active_step: Option<(String, String)>,
    visuals_blocked: bool,
}

impl FlowRunner {
    pub fn new(client: Option<Box<dyn Mcp>>, directory: PathBuf) -> Self {
        Self {
            client,
            tools: BTreeMap::new(),
            profile: Profile::default(),
            device: None,
            directory,
            values: BTreeMap::new(),
            secrets: Vec::new(),
            recording: false,
            query_count: 0,
            steps: Vec::new(),
            limitations: Vec::new(),
            version: 2,
            active_step: None,
            visuals_blocked: false,
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
            let steps = schema::compile(flow, profile)?;
            collect_secrets(&serde_json::to_value(&steps).unwrap(), &mut self.secrets);
            if !dry_run {
                for condition in &preconditions {
                    if !confirmed.contains(condition) {
                        return Err(format!(
                            "setup precondition not confirmed: {condition} (use --confirm after completing it outside the runner)"
                        ));
                    }
                }
                fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
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

    fn preflight(&self, steps: &[Step]) -> Result<()> {
        for step in steps {
            let (action, args) = step.action.iter().next().ok_or("empty action")?;
            let mut calls = Vec::new();
            let d = json!({"deviceId":"<selected>"});
            match action.as_str() {
                "select_device" => {
                    for key in args
                        .as_object()
                        .ok_or("device selector must be a mapping")?
                        .keys()
                    {
                        required(args, key)?;
                    }
                    if args
                        .get("platform")
                        .and_then(Value::as_str)
                        .is_some_and(|p| {
                            !["android", "ios", "macos", "windows", "chrome"].contains(&p)
                        })
                    {
                        return Err("unknown device platform".into());
                    }
                    calls.push(("mobile_list_available_devices", json!({})))
                }
                "launch_app" => calls.push((
                    "mobile_launch_app",
                    json!({"deviceId":"<selected>","appId":required(args,"app_id")?}),
                )),
                "open_url" => {
                    // Current MCP open_url is Chrome navigation only. A v1 app_id
                    // cannot be represented faithfully, so fail instead of drop it.
                    if args.get("app_id").is_some() {
                        return Err(
                            "open_url.app_id has no equivalent in the current MCP schema".into(),
                        );
                    }
                    calls.push((
                        "mobile_open_url",
                        json!({"deviceId":"<selected>","url":required(args,"url")?}),
                    ));
                }
                "screenshot" => {
                    self.artifact_path(required(args, "path")?)?;
                    calls.push(("mobile_take_screenshot", d.clone()));
                }
                "start_screen_recording" => {
                    calls.push(("mobile_start_screen_recording", d.clone()))
                }
                "stop_screen_recording" => calls.push(("mobile_stop_screen_recording", d.clone())),
                "tap" => calls.push((
                    TAP,
                    json!({"deviceId":"<selected>","x":number(args,"x")?,"y":number(args,"y")?}),
                )),
                "swipe" => calls.push((SWIPE, swipe_arguments(args, "<selected>")?)),
                "type_text" => calls.push((
                    TYPE,
                    json!({"deviceId":"<selected>","text":required(args,"text")?}),
                )),
                "call_tool" => {
                    let name = required(args, "name")?;
                    if [
                        "mobile_start_screen_recording",
                        "mobile_stop_screen_recording",
                    ]
                    .contains(&name)
                    {
                        return Err("use dedicated recording steps so cleanup is tracked".into());
                    }
                    calls.push((
                        name,
                        args.get("arguments")
                            .cloned()
                            .ok_or("call_tool.arguments missing")?,
                    ));
                }
                "tap_element" | "focus_field" | "fill_field" | "assert_element"
                | "assert_absent" | "assert_state" | "assert_value" | "assert_count"
                | "gesture" | "scroll" | "list_elements" => {
                    let s = schema::selector(args, &self.profile)?;
                    calls.push((QUERY, query_arguments(&s, "<selected>")));
                    if ["tap_element", "focus_field", "fill_field"].contains(&action.as_str()) {
                        calls.push(if s.native_tap {(BATCH,json!({"deviceId":"<selected>","actions":[{"type":"tap_element","filter":s.filter.as_ref().or(s.label.as_ref()).ok_or("native tap requires a filter or label")?}]}))}
                        else {(TAP,json!({"deviceId":"<selected>","x":0,"y":0}))});
                    }
                    if action == "fill_field" {
                        calls.push((
                            TYPE,
                            json!({"deviceId":"<selected>","text":required(args,"text")?}),
                        ));
                    }
                    if action == "assert_value" {
                        string(args, "expected")?;
                    }
                    if action == "assert_state" {
                        if !["checked", "selected", "focused", "enabled"]
                            .contains(&required(args, "property")?)
                        {
                            return Err("unknown assertion state".into());
                        }
                        if !args["expected"].is_boolean() {
                            return Err("state expected must be boolean".into());
                        }
                    }
                    if action == "assert_count" && args["count"].as_u64().is_none() {
                        return Err("assert_count.count must be an integer".into());
                    }
                    if action == "gesture" {
                        calls.push((
                            SWIPE,
                            gesture_arguments(
                                args,
                                &json!({"bounds":{"left":0,"top":0,"right":100,"bottom":100}}),
                                "<selected>",
                            )?,
                        ));
                    }
                    if action == "scroll"
                        && args
                            .get("direction")
                            .and_then(Value::as_str)
                            .is_some_and(|s| !["up", "down"].contains(&s))
                    {
                        return Err("scroll direction must be up or down".into());
                    }
                    if action == "gesture" || action == "scroll" || self.profile.viewport.is_some()
                    {
                        calls.push((SWIPE,json!({"deviceId":"<selected>","startX":0,"startY":1,"endX":0,"endY":0,"durationMs":350})));
                    }
                    for key in ["replacement", "verification"] {
                        if let Some(children) = args.get(key) {
                            self.preflight(
                                &serde_json::from_value::<Vec<Step>>(children.clone())
                                    .map_err(|e| e.to_string())?,
                            )?;
                        }
                    }
                }
                "if_element" => {
                    let s = schema::selector(&json!({"target":args["target"]}), &self.profile)?;
                    calls.push((QUERY, query_arguments(&s, "<selected>")));
                    for branch in ["then", "else"] {
                        self.preflight(
                            &serde_json::from_value::<Vec<Step>>(args[branch].clone())
                                .map_err(|e| e.to_string())?,
                        )?;
                    }
                }
                "note_limitation" => {
                    required(args, "message")?;
                }
                "wait" | "reset_fields" => {}
                _ => return Err(format!("unsupported action {action}")),
            }
            if !self.tools.is_empty() || self.client.is_some() {
                for (name, args) in calls {
                    let schema = self
                        .tools
                        .get(name)
                        .ok_or_else(|| format!("required MCP tool unavailable: {name}"))?;
                    mcp::validate_arguments(schema, &args).map_err(|e| format!("{name}: {e}"))?;
                }
            }
        }
        Ok(())
    }

    fn execute_steps<'a>(
        &'a mut self,
        steps: &'a [Step],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let parent = self.active_step.clone();
            for step in steps {
                let (action, args) = step.action.iter().next().unwrap();
                self.active_step = Some((step.id.clone().unwrap_or_default(), action.clone()));
                let result = timeout(
                    Duration::from_millis(
                        args.get("timeout_ms")
                            .and_then(Value::as_u64)
                            .unwrap_or(120_000),
                    ),
                    self.execute(action, args),
                )
                .await
                .unwrap_or_else(|_| Err("step timeout".into()));
                self.steps.push(StepReport {
                    id: step.id.clone().unwrap_or_default(),
                    action: action.clone(),
                    status: if result.is_err() {
                        "failed"
                    } else if self.client.is_none() {
                        "validated"
                    } else if self.visuals_blocked
                        && ["screenshot", "stop_screen_recording"].contains(&action.as_str())
                    {
                        "omitted"
                    } else {
                        "passed"
                    }
                    .into(),
                    error: result.as_ref().err().map(|e| self.redact(e)),
                });
                if result.is_ok() {
                    self.active_step = parent.clone();
                }
                result.map_err(|e| {
                    format!("step {} ({action}): {e}", step.id.as_deref().unwrap_or("?"))
                })?;
            }
            Ok(())
        })
    }

    async fn execute(&mut self, action: &str, args: &Value) -> Result<()> {
        if self.client.is_none() {
            return Ok(());
        }
        if args.get("sensitive") == Some(&json!(true)) {
            self.suspend_visuals().await?;
        }
        match action {
            "select_device" => {
                let result = self
                    .call("mobile_list_available_devices", json!({}))
                    .await?;
                let devices = result
                    .pointer("/structuredContent/devices")
                    .and_then(Value::as_array)
                    .ok_or("MCP devices missing")?;
                let matches = devices
                    .iter()
                    .filter(|d| {
                        args.as_object()
                            .is_some_and(|o| o.iter().all(|(k, v)| d.get(k) == Some(v)))
                    })
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(format!(
                        "device selection matched {} devices; require exactly one",
                        matches.len()
                    ));
                }
                self.device = Some(matches[0].clone());
            }
            "wait" => {
                sleep(Duration::from_millis(
                    args["ms"].as_u64().ok_or("invalid wait.ms")?,
                ))
                .await
            }
            "note_limitation" => self.limitations.push(required(args, "message")?.into()),
            "reset_fields" => self.values.clear(),
            "if_element" => {
                let s = schema::selector(&json!({"target":args["target"]}), &self.profile)?;
                let found = self.query(&s).await?;
                if found.len() > 1 {
                    return Err("ambiguous conditional selector".into());
                }
                if found.first().is_some_and(|e| e["enabled"] != true) {
                    return Err("conditional element is disabled".into());
                }
                let branch = if found.is_empty() { "else" } else { "then" };
                let children: Vec<Step> =
                    serde_json::from_value(args[branch].clone()).map_err(|e| e.to_string())?;
                self.execute_steps(&children).await?;
            }
            "launch_app" => {
                self.call(
                    "mobile_launch_app",
                    json!({"deviceId":self.device_id()?,"appId":required(args,"app_id")?}),
                )
                .await?;
                self.values.clear();
            }
            "open_url" => {
                self.call(
                    "mobile_open_url",
                    json!({"deviceId":self.device_id()?,"url":required(args,"url")?}),
                )
                .await?;
            }
            "type_text" => {
                self.call(
                    TYPE,
                    json!({"deviceId":self.device_id()?,"text":required(args,"text")?}),
                )
                .await?;
            }
            "tap" => {
                self.call(TAP,json!({"deviceId":self.device_id()?,"x":number(args,"x")?,"y":number(args,"y")?})).await?;
            }
            "swipe" => {
                self.call(SWIPE, swipe_arguments(args, self.device_id()?)?)
                    .await?;
            }
            "call_tool" => {
                let mut arguments = args["arguments"].clone();
                if arguments.get("deviceId") == Some(&json!("<selected>")) {
                    arguments["deviceId"] = json!(self.device_id()?);
                }
                self.call(required(args, "name")?, arguments).await?;
            }
            "start_screen_recording" => {
                if self.visuals_blocked {
                    return Err("visual recording is disabled after sensitive input".into());
                }
                if self.recording {
                    return Err("recording already started".into());
                }
                self.call(
                    "mobile_start_screen_recording",
                    json!({"deviceId":self.device_id()?}),
                )
                .await?;
                self.recording = true;
            }
            "stop_screen_recording" => {
                if !self.visuals_blocked {
                    self.stop_recording().await?;
                }
            }
            "screenshot" => {
                if self.visuals_blocked {
                    return Ok(());
                }
                let path = self.artifact_path(required(args, "path")?)?;
                let result = self
                    .call(
                        "mobile_take_screenshot",
                        json!({"deviceId":self.device_id()?}),
                    )
                    .await?;
                let image = result["content"]
                    .as_array()
                    .ok_or("screenshot content missing")?
                    .iter()
                    .find(|c| c["type"] == "image" && c["mimeType"] == "image/png")
                    .ok_or("PNG screenshot missing")?;
                let bytes = STANDARD
                    .decode(image["data"].as_str().ok_or("image data missing")?)
                    .map_err(|e| e.to_string())?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err("invalid PNG screenshot".into());
                }
                write(&path, &bytes)?;
            }
            "scroll" => {
                let selector = schema::selector(args, &self.profile)?;
                self.scroll(
                    &selector,
                    args.get("direction")
                        .and_then(Value::as_str)
                        .unwrap_or("down"),
                )
                .await?;
            }
            "gesture" => {
                let element = self.find(args, false).await?;
                self.call(SWIPE, gesture_arguments(args, &element, self.device_id()?)?)
                    .await?;
            }
            "tap_element" | "focus_field" => {
                self.focus(args).await?;
            }
            "fill_field" => {
                let s = schema::selector(args, &self.profile)?;
                let e = self.focus(args).await?;
                let key = args
                    .get("target")
                    .and_then(Value::as_str)
                    .ok_or("fill_field requires logical target")?;
                let existing = self.values.get(key).cloned().or_else(|| {
                    e.get("value")
                        .and_then(Value::as_str)
                        .or_else(|| e.get("text").and_then(Value::as_str))
                        .filter(|v| !v.is_empty() && s.label.as_deref() != Some(*v))
                        .map(str::to_owned)
                });
                if existing.as_deref().is_some_and(|v| !v.is_empty()) {
                    let replacement = args
                        .get("replacement")
                        .ok_or("nonempty field requires an explicit profile replacement recipe")?;
                    let children: Vec<Step> =
                        serde_json::from_value(replacement.clone()).map_err(|e| e.to_string())?;
                    self.execute_steps(&children).await?;
                } else {
                    self.call(
                        TYPE,
                        json!({"deviceId":self.device_id()?,"text":required(args,"text")?}),
                    )
                    .await?;
                }
                if let Some(verification) = args.get("verification") {
                    self.execute_steps(
                        &serde_json::from_value::<Vec<Step>>(verification.clone())
                            .map_err(|e| e.to_string())?,
                    )
                    .await?;
                } else {
                    self.assert_value(args, required(args, "text")?).await?;
                }
                self.values
                    .insert(key.into(), required(args, "text")?.into());
            }
            "assert_absent" | "assert_count" | "list_elements" => {
                let s = schema::selector(args, &self.profile)?;
                let found = self.query(&s).await?;
                if action == "assert_absent" && !found.is_empty() {
                    return Err("unexpected element in current accessibility tree".into());
                }
                if action == "assert_count" && args["count"].as_u64() != Some(found.len() as u64) {
                    return Err(format!(
                        "expected {} element(s), observed {}",
                        args["count"],
                        found.len()
                    ));
                }
            }
            "assert_element" => {
                self.find_with_uniqueness(args, false, false).await?;
            }
            "assert_state" => {
                let e = self.find(args, false).await?;
                let property = required(args, "property")?;
                if e.get(property).and_then(Value::as_bool).is_none() {
                    return Err(format!("{property} is unknown, not false"));
                }
                if e[property] != args["expected"] {
                    return Err(format!("{property} does not match expected state"));
                }
            }
            "assert_value" => self.assert_value(args, string(args, "expected")?).await?,
            _ => return Err(format!("unknown action {action}")),
        }
        Ok(())
    }

    async fn assert_value(&mut self, args: &Value, expected: &str) -> Result<()> {
        let e = self.find(args, false).await?;
        // A native value is authoritative, even when its label matches the
        // expectation. Null is unknown; a label cannot supply that missing value.
        if let Some(value) = e.get("value") {
            return match value.as_str() {
                Some(value) if value == expected => Ok(()),
                Some(_) => Err("field does not expose expected value".into()),
                None => Err("Unsupported: native field value is unknown".into()),
            };
        }
        let exposed = ["text", "contentDescription"]
            .into_iter()
            .filter_map(|key| e.get(key).and_then(Value::as_str))
            .collect::<Vec<_>>();
        if exposed.is_empty() {
            return Err("Unsupported: native field value is unknown".into());
        }
        // Existing native responses may combine a field's text and label on
        // separate lines. Retain that baseline representation when value is absent.
        if !exposed
            .iter()
            .any(|v| *v == expected || v.lines().any(|line| line == expected))
        {
            return Err("field does not expose expected value".into());
        }
        Ok(())
    }

    async fn find(&mut self, args: &Value, tap: bool) -> Result<Value> {
        self.find_with_uniqueness(args, tap, true).await
    }

    async fn find_with_uniqueness(
        &mut self,
        args: &Value,
        tap: bool,
        unique: bool,
    ) -> Result<Value> {
        let s = schema::selector(args, &self.profile)?;
        let deadline = Instant::now()
            + Duration::from_millis(
                args.get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(30_000),
            );
        let mut previous = Value::Null;
        let mut stable = 0;
        let mut attempts = 0;
        loop {
            let mut found = self.query(&s).await?;
            if tap {
                let controls = found
                    .iter()
                    .filter(|e| e["clickable"] == true)
                    .cloned()
                    .collect::<Vec<_>>();
                if !controls.is_empty() {
                    found = controls;
                }
            }
            if unique && found.len() > 1 {
                return Err(format!(
                    "ambiguous selector: {} matching elements",
                    found.len()
                ));
            }
            // Native trees can retain offscreen controls with an all-zero
            // rectangle. Search the viewport for those controls; malformed
            // geometry and ambiguity remain errors, not selector fallbacks.
            found.retain(|e| {
                !["left", "top", "right", "bottom"]
                    .iter()
                    .all(|key| e["bounds"][key].as_f64() == Some(0.))
            });
            if let Some(e) = found.first() {
                let bounds = rectangle(e)?;
                if !tap || (e["enabled"] == true && e["clickable"] == true) {
                    // Unselected resource IDs may be snapshot-local handles.
                    // Stability tracks the observed selector/geometry, not those
                    // handles. Explicitly selected IDs remain part of the match.
                    let selected_id = s
                        .resource_id
                        .as_ref()
                        .or(s.prefer_resource_id.as_ref())
                        .filter(|id| {
                            e.get("resourceId").and_then(Value::as_str) == Some(id.as_str())
                        });
                    let identity = json!([
                        bounds,
                        selected_id,
                        e.get("className"),
                        e.get("text"),
                        e.get("contentDescription")
                    ]);
                    if identity == previous {
                        stable += 1;
                    } else {
                        stable = 1;
                        previous = identity;
                    }
                    if stable
                        >= s.stable_samples
                            .or(self.profile.stable_samples)
                            .unwrap_or(1)
                    {
                        return Ok(e.clone());
                    }
                }
            } else {
                stable = 0;
                previous = Value::Null;
                if (1..11).contains(&attempts) {
                    if let Some(viewport) = self.profile.viewport.clone() {
                        let viewport = self
                            .profile
                            .selectors
                            .get(&viewport)
                            .cloned()
                            .ok_or("unknown viewport selector")?;
                        // Match the native shell search: four down, then six up.
                        // The return leg must cross the original position to
                        // find content above it, rather than merely retrace it.
                        self.scroll(&viewport, if attempts < 5 { "down" } else { "up" })
                            .await?;
                    }
                }
            }
            attempts += 1;
            if Instant::now() >= deadline || attempts >= 12 {
                return Err(
                    "element missing, disabled, or unstable before bounded wait expired".into(),
                );
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    fn query<'a>(
        &'a mut self,
        s: &'a Selector,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>>> + Send + 'a>> {
        Box::pin(async move {
            let result = self
                .call(QUERY, query_arguments(s, self.device_id()?))
                .await?;
            let raw = result
                .pointer("/structuredContent/elements")
                .and_then(Value::as_array)
                .ok_or("MCP elements missing")?;
            if raw.len() >= s.max_results.unwrap_or(100) {
                return Err("query reached max_results; result may be truncated".into());
            }
            self.query_count += 1;
            let mut trace = json!(raw);
            self.scrub(&mut trace);
            write(
                &self
                    .directory
                    .join(format!("query-{}.json", self.query_count)),
                serde_json::to_string_pretty(&trace).unwrap().as_bytes(),
            )?;
            let mut found = filter_elements(s, raw);
            for (relation, exclude) in [(&s.row_with, false), (&s.exclude_row_with, true)] {
                if let Some(name) = relation {
                    let anchor = self
                        .profile
                        .selectors
                        .get(name)
                        .cloned()
                        .ok_or("unknown row anchor")?;
                    let anchors = self.query(&anchor).await?;
                    if anchors.is_empty() && s.allow_missing_row_anchor {
                        continue;
                    }
                    if anchors.len() != 1 {
                        return Err("row anchor must match exactly one element".into());
                    }
                    let b = rectangle(&anchors[0])?;
                    let y = (b.1 + b.3) / 2.;
                    found.retain(|e| rectangle(e).is_ok_and(|b| (y >= b.1 && y <= b.3) != exclude));
                }
            }
            Ok(found)
        })
    }

    async fn focus(&mut self, args: &Value) -> Result<Value> {
        let s = schema::selector(args, &self.profile)?;
        for _ in 0..3 {
            let e = self.find(args, true).await?;
            self.tap_element(&s, &e).await?;
            if !s.require_focus {
                return Ok(e);
            }
            let focused = self.find(args, false).await?;
            if focused.get("focused").and_then(Value::as_bool) == Some(true) {
                return Ok(focused);
            }
        }
        Err("editor focus was not observed".into())
    }

    async fn tap_element(&mut self, s: &Selector, e: &Value) -> Result<()> {
        if s.native_tap {
            let filter = s
                .filter
                .as_ref()
                .or(s.label.as_ref())
                .ok_or("native tap requires label/filter")?;
            self.call(BATCH,json!({"deviceId":self.device_id()?,"actions":[{"type":"tap_element","filter":filter}]})).await?;
        } else {
            let b = rectangle(e)?;
            self.call(
                TAP,
                json!({"deviceId":self.device_id()?,"x":(b.0+b.2)/2.,"y":(b.1+b.3)/2.}),
            )
            .await?;
        }
        Ok(())
    }

    async fn scroll(&mut self, s: &Selector, direction: &str) -> Result<()> {
        if !["up", "down"].contains(&direction) {
            return Err("scroll direction must be up or down".into());
        }
        let found = self.query(s).await?;
        if found.len() != 1 {
            return Err("scroll viewport must match exactly one element".into());
        }
        let (from, to) = if direction == "down" {
            (0.72, 0.4)
        } else {
            (0.4, 0.72)
        };
        let args = json!({"from":{"x":0.5,"y":from},"to":{"x":0.5,"y":to},"duration_ms":350});
        self.call(
            SWIPE,
            gesture_arguments(&args, &found[0], self.device_id()?)?,
        )
        .await?;
        Ok(())
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

    async fn stop_recording(&mut self) -> Result<()> {
        if !self.recording {
            return Err("no active recording".into());
        }
        self.recording = false;
        let result = self
            .call(
                "mobile_stop_screen_recording",
                json!({"deviceId":self.device_id()?}),
            )
            .await?;
        let path = result["content"]
            .as_array()
            .and_then(|c| {
                c.iter()
                    .filter_map(|v| v["text"].as_str())
                    .find_map(|t| t.strip_prefix("Screen recording saved: "))
            })
            .ok_or("recorded video path missing")?;
        let path = Path::new(path.trim());
        if fs::metadata(path).map_err(|e| e.to_string())?.len() == 0 {
            return Err("recorded video is empty".into());
        }
        fs::copy(path, self.directory.join("video.mp4")).map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn suspend_visuals(&mut self) -> Result<()> {
        if self.visuals_blocked {
            return Ok(());
        }
        if self.recording {
            self.stop_recording().await?;
        }
        self.visuals_blocked = true;
        self.limitations.push("Visual evidence stopped/omitted before sensitive or environment-sourced input; remaining actions have redacted query/report evidence only.".into());
        Ok(())
    }

    fn device_id(&self) -> Result<&str> {
        self.device
            .as_ref()
            .and_then(|d| d["id"].as_str())
            .ok_or("select a device before UI actions".into())
    }
    fn artifact_path(&self, name: &str) -> Result<PathBuf> {
        let path = Path::new(name);
        if self.version == 1 {
            return Ok(path.to_path_buf());
        }
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err("artifact path must stay inside artifact directory".into());
        }
        Ok(self.directory.join(path))
    }
    fn redact(&self, text: &str) -> String {
        let mut result = text.to_owned();
        let mut secrets = self.secrets.clone();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for secret in secrets {
            if !secret.is_empty() {
                result = result.replace(&secret, "<REDACTED>");
            }
        }
        result
    }
    fn scrub(&self, value: &mut Value) {
        match value {
            Value::String(t) => *t = self.redact(t),
            Value::Array(items) => {
                for v in items {
                    self.scrub(v)
                }
            }
            Value::Object(items) => {
                for v in items.values_mut() {
                    self.scrub(v)
                }
            }
            _ => {}
        }
    }
}

pub fn filter_elements(s: &Selector, raw: &[Value]) -> Vec<Value> {
    let mut found = raw
        .iter()
        .filter(|e| {
            let labels = ["text", "contentDescription"]
                .iter()
                .filter_map(|k| e[*k].as_str())
                .flat_map(str::lines)
                .map(str::to_lowercase)
                .collect::<Vec<_>>();
            s.label
                .as_ref()
                .is_none_or(|l| labels.contains(&l.to_lowercase()))
                && (s.labels.is_empty()
                    || s.labels.iter().any(|l| labels.contains(&l.to_lowercase())))
                && s.element_type
                    .as_ref()
                    .is_none_or(|t| e["elementType"] == *t)
                && s.resource_id.as_ref().is_none_or(|r| e["resourceId"] == *r)
                && s.class_contains
                    .as_ref()
                    .is_none_or(|c| e["className"].as_str().is_some_and(|v| v.contains(c)))
        })
        .cloned()
        .collect::<Vec<_>>();
    if let Some(preferred) = &s.prefer_resource_id {
        let keyed = found
            .iter()
            .filter(|e| e["resourceId"] == *preferred)
            .cloned()
            .collect::<Vec<_>>();
        if !keyed.is_empty() {
            found = keyed;
        }
    }
    if s.largest {
        if let Some(e) = found
            .iter()
            .filter_map(|e| rectangle(e).ok().map(|b| (e, (b.2 - b.0) * (b.3 - b.1))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
        {
            return vec![e.0.clone()];
        }
    }
    found
}

fn query_arguments(s: &Selector, device: &str) -> Value {
    let mut args =
        json!({"deviceId":device,"maxResults":s.max_results.unwrap_or(100),"timeoutMs":60000});
    if let Some(filter) = s.filter.as_ref().or(s.label.as_ref()) {
        args["filter"] = json!(filter);
    }
    if let Some(t) = &s.element_type {
        args["elementType"] = json!(t);
    }
    args
}
fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing or empty {key}"))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string {key}"))
}
fn number(v: &Value, key: &str) -> Result<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("invalid coordinate {key}"))
}
fn rectangle(v: &Value) -> Result<(f64, f64, f64, f64)> {
    let b = &v["bounds"];
    let r = (
        number(b, "left")?,
        number(b, "top")?,
        number(b, "right")?,
        number(b, "bottom")?,
    );
    if r.0 < 0. || r.1 < 0. || r.2 <= r.0 || r.3 <= r.1 {
        return Err("element has no visible valid bounds".into());
    }
    Ok(r)
}
fn gesture_arguments(args: &Value, e: &Value, device: &str) -> Result<Value> {
    let b = rectangle(e)?;
    let x = |n: f64| b.0 + (b.2 - b.0) * n;
    let y = |n: f64| b.1 + (b.3 - b.1) * n;
    let (sx, sy, ex, ey) = (
        number(&args["from"], "x")?,
        number(&args["from"], "y")?,
        number(&args["to"], "x")?,
        number(&args["to"], "y")?,
    );
    if [sx, sy, ex, ey]
        .into_iter()
        .any(|n| !(0.0..=1.0).contains(&n))
    {
        return Err("gesture fractions must be 0..1".into());
    }
    Ok(
        json!({"deviceId":device,"startX":x(sx),"startY":y(sy),"endX":x(ex),"endY":y(ey),"durationMs":args.get("duration_ms").and_then(Value::as_u64).unwrap_or(350)}),
    )
}
fn swipe_arguments(args: &Value, device: &str) -> Result<Value> {
    Ok(
        json!({"deviceId":device,"startX":number(&args["start"],"x")?,"startY":number(&args["start"],"y")?,"endX":number(&args["end"],"x")?,"endY":number(&args["end"],"y")?,"durationMs":args.get("duration_ms").and_then(Value::as_u64).unwrap_or(300)}),
    )
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, bytes).map_err(|e| e.to_string())
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

fn collect_secrets(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for key in ["text", "expected"] {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    if !s.is_empty() {
                        out.push(s.into());
                    }
                }
            }
            for v in map.values() {
                collect_secrets(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_secrets(v, out)
            }
        }
        _ => {}
    }
}

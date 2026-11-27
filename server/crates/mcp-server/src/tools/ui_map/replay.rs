use std::{collections::BTreeMap, path::Path, sync::Arc};

use async_trait::async_trait;
use robot_core::{Robot, UiElement, WebContext};
use robot_flow::{
    FlowDocument, FlowReport, FlowRunner, Profile,
    mcp::{Mcp, Result as McpResult},
};
use serde_json::{Value, json};

use super::{
    MAX_IMAGE_BYTES,
    crawl::Scope,
    model::ActionEvidence,
    viewport::{Reveal, SCROLL_SETTLE_MS},
};
use crate::tools::flow::CaseMcp;

const DEVICE: &str = "DEVICE_BOT_MAP_DEVICE";
const INPUT: &str = "DEVICE_BOT_MAP_INPUT";

pub(super) struct Action<'a> {
    pub element: &'a UiElement,
    pub label: &'a str,
    pub resource_id: Option<&'a str>,
    pub input: Option<&'a str>,
    pub initially_offscreen: Option<bool>,
    pub flutter: bool,
    pub reveal: Option<&'a Reveal>,
}

fn stable_context(context: &WebContext) -> Value {
    json!({
        "app_id": context.app_id,
        "frame_path": context.frame_path.iter().map(|frame| json!({
            "css": frame.css,
            "name": frame.name,
            "url": frame.url,
            "position": frame.position
        })).collect::<Vec<_>>()
    })
}

fn escape_templates(value: &mut Value) {
    match value {
        Value::String(text) => *text = text.replace("${", "$${"),
        Value::Array(items) => items.iter_mut().for_each(escape_templates),
        Value::Object(items) => items.values_mut().for_each(escape_templates),
        _ => {}
    }
}

impl Action<'_> {
    fn yaml(
        &self,
        scope: &Scope,
        name: &str,
        settle_ms: u64,
        capture: bool,
    ) -> Result<String, String> {
        let mut selector = if matches!(scope, Scope::Mobile { .. } | Scope::IosNative) {
            // Use the flow runner's normal selector wait and the driver's
            // deadline. A short step override also cuts off the native tap.
            json!({})
        } else {
            json!({"timeout_ms":3000})
        };
        if self.flutter {
            selector["source"] = json!("flutter");
        }
        if self.reveal.is_some() {
            // Native wheel events can still be moving the target after the
            // first observation. Require stable geometry before the click.
            selector["stable_samples"] = json!(2);
        }
        if matches!(scope, Scope::Web { .. }) && self.initially_offscreen.is_some() {
            selector["scroll_into_view"] = json!(true);
            selector["timeout_ms"] = json!(8000);
        }
        if let Some(id) = self.resource_id {
            selector["resource_id"] = json!(id);
        } else {
            selector["label"] = json!(self.label);
        }
        if let Some(kind) = self.element.element_type {
            selector["element_type"] = json!(kind.as_str());
        }
        let context = self
            .element
            .web_context
            .as_ref()
            .filter(|ctx| ctx.app_id.is_some() || !ctx.frame_path.is_empty());
        if let Some(ctx) = context {
            selector["web_context"] = stable_context(ctx);
        }
        escape_templates(&mut selector);
        let mut steps = vec![json!({"select_device":{"id":format!("${{env:{DEVICE}}}")}})];
        if let Some(plan) = self.reveal {
            let mut scroll = json!({
                "class_contains": plan.anchor.locator.class_name,
                "source": if self.flutter { "flutter" } else { "native" },
            });
            let step = if let Some(gesture) = plan.anchor.android_swipe(plan.delta) {
                scroll["from"] = json!(gesture.from);
                scroll["to"] = json!(gesture.to);
                scroll["duration_ms"] = json!(gesture.duration_ms);
                "gesture"
            } else {
                scroll["direction"] = json!(if plan.delta > 0 { "down" } else { "up" });
                scroll["wheel_delta"] = json!(plan.delta.unsigned_abs());
                "scroll"
            };
            if let Some(id) = &plan.anchor.locator.resource_id {
                scroll["resource_id"] = json!(id);
            }
            escape_templates(&mut scroll);
            for _ in &plan.slices {
                steps.push(json!({(step):scroll}));
                steps.push(json!({"wait":{"ms":SCROLL_SETTLE_MS}}));
            }
        }
        let note = self
            .initially_offscreen
            .filter(|outside| *outside)
            .map(|_| "The control was outside the viewport when this screen was inventoried.");
        if self.input.is_some() {
            selector["require_focus"] = json!(true);
            let mut clear = json!({"deviceId":"<selected>","text":"","clear":true});
            let mut typing = json!({"text":format!("${{env:{INPUT}}}"),"sensitive":true});
            if let Some(ctx) = context {
                let mut stable = ctx.clone();
                stable.element_ref = None;
                stable.page = None;
                clear["webContext"] = json!(stable);
                escape_templates(&mut clear);
                typing["web_context"] = selector["web_context"].clone();
            }
            steps.push(action_step("focus_field", selector, note));
            steps.push(json!({"call_tool":{"name":"mobile_type_keys","arguments":clear}}));
            steps.push(json!({"type_text":typing}));
        } else {
            steps.push(action_step("tap_element", selector, note));
        }
        steps.push(json!({"wait":{"ms":settle_ms}}));
        if capture {
            steps.push(json!({"screenshot":{"path":"result.png"}}));
        }
        serde_yaml::to_string(&json!({"version":2,"name":name,"steps":steps}))
            .map_err(|error| error.to_string())
    }
}

fn action_step(action: &str, selector: Value, note: Option<&str>) -> Value {
    let mut step = json!({(action):selector});
    if let Some(note) = note {
        step["note"] = json!(note);
    }
    step
}

struct ScopedMcp {
    session: CaseMcp,
    robot: Arc<dyn Robot>,
    scope: Scope,
    device: String,
}

#[async_trait]
impl Mcp for ScopedMcp {
    async fn request(&mut self, method: &str, params: Value) -> McpResult<Value> {
        if method == "tools/call" && params["name"] != "mobile_list_available_devices" {
            self.scope.location(&*self.robot, &self.device).await?;
        }
        self.session.request(method, params).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> McpResult<()> {
        self.session.notify(method, params).await
    }

    async fn close(&mut self) -> McpResult<()> {
        self.session.close().await
    }
}

pub(super) async fn run(
    robot: &Arc<dyn Robot>,
    scope: &Scope,
    device: &str,
    dir: &Path,
    name: &str,
    action: Action<'_>,
    settle_ms: u64,
) -> Result<(ActionEvidence, Option<String>), String> {
    let relative = format!("actions/{name}");
    let directory = dir.join(&relative);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| e.to_string())?;
    let yaml = action.yaml(scope, name, settle_ms, true)?;
    let yaml_path = format!("{relative}/scenario.yaml");
    tokio::fs::write(dir.join(&yaml_path), &yaml)
        .await
        .map_err(|e| e.to_string())?;

    let scratch = directory.join(".runner");
    let report = execute(robot, scope, device, &scratch, &yaml, action.input).await?;
    let screenshot = scratch.join("result.png");
    let mut evidence = ActionEvidence {
        yaml: yaml_path,
        screenshot: None,
    };
    if tokio::fs::metadata(&screenshot)
        .await
        .is_ok_and(|meta| meta.len() <= MAX_IMAGE_BYTES as u64)
    {
        tokio::fs::rename(screenshot, directory.join("result.png"))
            .await
            .map_err(|e| e.to_string())?;
        evidence.screenshot = Some(format!("{relative}/result.png"));
    }
    let report_json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    tokio::fs::write(directory.join("report.json"), report_json)
        .await
        .map_err(|e| e.to_string())?;
    if let Err(error) = tokio::fs::remove_dir_all(&scratch).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!(
            "Could not remove temporary action diagnostics: {error}"
        ));
    }
    let error = report.error.or_else(|| {
        evidence
            .screenshot
            .is_none()
            .then(|| "Action screenshot was missing or exceeded the size limit.".into())
    });
    Ok((evidence, error))
}

pub(super) async fn restore(
    robot: &Arc<dyn Robot>,
    scope: &Scope,
    device: &str,
    dir: &Path,
    action: Action<'_>,
    settle_ms: u64,
) -> Result<(), String> {
    let yaml = action.yaml(scope, "restore_ui_map_branch", settle_ms, false)?;
    let scratch = dir.join(".restore");
    let report = execute(robot, scope, device, &scratch, &yaml, action.input).await?;
    tokio::fs::remove_dir_all(scratch)
        .await
        .map_err(|e| e.to_string())?;
    report.error.map_or(Ok(()), Err)
}

async fn execute(
    robot: &Arc<dyn Robot>,
    scope: &Scope,
    device: &str,
    scratch: &Path,
    yaml: &str,
    input: Option<&str>,
) -> Result<FlowReport, String> {
    let flow = FlowDocument::from_yaml(yaml).map_err(|e| e.to_string())?;
    let client = ScopedMcp {
        session: CaseMcp::for_existing_session(robot.clone()),
        robot: robot.clone(),
        scope: scope.clone(),
        device: device.to_owned(),
    };
    let mut env = BTreeMap::from([(DEVICE.to_owned(), device.to_owned())]);
    if let Some(value) = input {
        env.insert(INPUT.to_owned(), value.to_owned());
    }
    let mut runner = FlowRunner::new(Some(Box::new(client)), scratch.to_owned());
    runner.set_environment(env);
    Ok(runner
        .run_without_case_timeout(&flow, &Profile::default(), &[])
        .await)
}

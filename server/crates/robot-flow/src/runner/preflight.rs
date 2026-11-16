//! Validate the complete compiled flow before executing any tool calls.

use super::arguments::{
    app_launch_call, gesture_arguments, number, query_arguments, required, route_tool_arguments,
    string, swipe_arguments, type_arguments,
};
use super::{BATCH, FlowRunner, QUERY, SWIPE, TAP, TYPE};
use crate::{
    mcp::{self, Result},
    schema::{self, Step},
};
use serde_json::{Value, json};

impl FlowRunner {
    pub(super) fn preflight(&self, steps: &[Step]) -> Result<()> {
        self.preflight_scoped(steps, false)
    }

    pub(super) fn preflight_scoped(&self, steps: &[Step], inherited_platform: bool) -> Result<()> {
        for step in steps {
            let (action, args) = step.action.iter().next().ok_or("empty action")?;
            let mut calls = Vec::new();
            let scoped = inherited_platform || step.platform.is_some() || step.resource.is_some();
            if let Some(name) = &step.resource {
                let device = self
                    .named_resources
                    .get(name)
                    .ok_or_else(|| format!("unknown step resource {name}"))?;
                if device
                    .get("id")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.trim().is_empty())
                    || device
                        .get("platform")
                        .and_then(Value::as_str)
                        .is_none_or(|v| {
                            !["android", "ios", "macos", "windows", "chrome"].contains(&v)
                        })
                {
                    return Err(format!("invalid device binding for resource {name}: expected nonempty id and supported platform").into());
                }
            }
            if scoped && action == "select_device" {
                return Err(
                    "select_device cannot change the default device inside a platform-scoped step"
                        .into(),
                );
            }
            if step.platform.is_some() {
                calls.push(("mobile_list_available_devices", json!({})));
            }
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
                "launch_app" => {
                    calls.push(app_launch_call("<selected>", required(args, "app_id")?))
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
                    type_arguments("<selected>", required(args, "text")?, args)?,
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
                    let mut arguments = args
                        .get("arguments")
                        .cloned()
                        .ok_or("call_tool.arguments missing")?;
                    if scoped {
                        route_tool_arguments(&mut arguments, "<selected>")?;
                    }
                    calls.push((name, arguments));
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
                            type_arguments("<selected>", required(args, "text")?, args)?,
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
                            self.preflight_scoped(
                                &serde_json::from_value::<Vec<Step>>(children.clone())
                                    .map_err(|e| e.to_string())?,
                                scoped,
                            )?;
                        }
                    }
                }
                "if_element" => {
                    let s = schema::selector(&json!({"target":args["target"]}), &self.profile)?;
                    calls.push((QUERY, query_arguments(&s, "<selected>")));
                    for branch in ["then", "else"] {
                        self.preflight_scoped(
                            &serde_json::from_value::<Vec<Step>>(args[branch].clone())
                                .map_err(|e| e.to_string())?,
                            scoped,
                        )?;
                    }
                }
                "note_limitation" => {
                    required(args, "message")?;
                }
                "wait" | "reset_fields" => {}
                _ => return Err(format!("unsupported action {action}").into()),
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
}

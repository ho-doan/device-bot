//! Dispatch validated flow actions through the runner MCP interface.

use super::arguments::{
    app_launch_call, gesture_arguments, number, pointer_gesture_arguments, rectangle, required,
    route_tool_arguments, string, swipe_arguments, swipe_element_arguments,
};
use super::artifacts::write;
use super::{FlowRunner, SWIPE, StepEvidence, TAP, TYPE};
use crate::{
    mcp::Result,
    schema::{self, Step},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

fn macos_window_id(id: &str) -> bool {
    let Some((pid, window)) = id.strip_prefix("macos:").and_then(|id| id.split_once(':')) else {
        return false;
    };
    pid.bytes().all(|b| b.is_ascii_digit())
        && window.bytes().all(|b| b.is_ascii_digit())
        && pid.parse::<i32>().is_ok_and(|pid| pid > 0)
        && window.parse::<u32>().is_ok_and(|window| window > 0)
}

impl FlowRunner {
    pub(super) async fn execute(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Result<Option<StepEvidence>> {
        if self.client.is_none() {
            return Ok(None);
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
                let window = args["id"].as_str().filter(|id| macos_window_id(id));
                let matches = devices
                    .iter()
                    .filter_map(|d| {
                        let mut device = d.clone();
                        if let Some(id) = window
                            && d["id"] == "macos-host"
                            && d["platform"] == "macos"
                            && d["state"] == "available"
                        {
                            // The driver validates the PID/window pair before
                            // inspecting, capturing, or sending input.
                            device["id"] = json!(id);
                        }
                        args.as_object()
                            .is_some_and(|o| o.iter().all(|(k, v)| device.get(k) == Some(v)))
                            .then_some(device)
                    })
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(format!(
                        "device selection matched {} devices; require exactly one",
                        matches.len()
                    )
                    .into());
                }
                self.device = matches.into_iter().next();
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
                let (tool, arguments) =
                    app_launch_call(self.device_id()?, required(args, "app_id")?);
                self.call(tool, arguments).await?;
                self.values.clear();
            }
            "type_text" => {
                self.call(TYPE, self.type_payload(required(args, "text")?, args)?)
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
                if self.step_device.is_some() {
                    route_tool_arguments(&mut arguments, self.device_id()?)?;
                } else if arguments.get("deviceId") == Some(&json!("<selected>")) {
                    arguments["deviceId"] = json!(self.device_id()?);
                }
                self.call(required(args, "name")?, arguments).await?;
            }
            "start_screen_recording" => {
                if self.recording {
                    return Err("recording already started".into());
                }
                let mut overlay = crate::video::VideoOverlay::new();
                if let Some((id, action)) = &self.active_step {
                    overlay.mark(id, action, args);
                }
                self.call(
                    "mobile_start_screen_recording",
                    json!({"deviceId":self.device_id()?}),
                )
                .await?;
                self.recording_device = Some(self.device_id()?.to_owned());
                self.recording = true;
                self.video_overlay = Some(overlay);
            }
            "stop_screen_recording" => {
                if self.step_device.is_some()
                    && self.recording_device.as_deref() != Some(self.device_id()?)
                {
                    return Err(
                        "stop_screen_recording platform does not match the recording device".into(),
                    );
                }
                self.stop_recording().await?;
            }
            "screenshot" => {
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
                if let Some(dy) = schema::scroll_wheel_delta(args)? {
                    self.scroll_wheel(&selector, dy).await?;
                } else {
                    self.scroll(
                        &selector,
                        args.get("direction")
                            .and_then(Value::as_str)
                            .unwrap_or("down"),
                    )
                    .await?;
                }
            }
            "gesture" => {
                let element = self.find(args, false).await?;
                if element
                    .get("webContext")
                    .is_some_and(|context| !context.is_null())
                {
                    return Err("DOM gestures require a verified native coordinate mapping; use scroll instead".into());
                }
                self.call(SWIPE, gesture_arguments(args, &element, self.device_id()?)?)
                    .await?;
            }
            "swipe_element" => {
                let element = self.find_gesture_target(args).await?;
                self.call(
                    SWIPE,
                    swipe_element_arguments(args, &element, self.device_id()?)?,
                )
                .await?;
            }
            "tap_element" | "focus_field" => {
                self.focus(args).await?;
            }
            "long_press_element" | "double_tap_element" => {
                let element = self.find_gesture_target(args).await?;
                let gesture = schema::pointer_gesture_step(action, args)?;
                self.call(
                    TAP,
                    pointer_gesture_arguments(
                        self.device_id()?,
                        &element,
                        rectangle(&element)?,
                        gesture,
                    ),
                )
                .await?;
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
                    self.call(TYPE, self.type_payload(required(args, "text")?, args)?)
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
            "list_elements" => {
                return self
                    .probe_elements(args)
                    .await
                    .map(|report| Some(StepEvidence::ElementProbe(report)));
            }
            "network_fault" => {
                return self
                    .run_web_fault(args)
                    .await
                    .map(|report| Some(StepEvidence::NetworkFault(report)));
            }
            "assert_absent" | "assert_count" => {
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
                    )
                    .into());
                }
            }
            "assert_element" => {
                self.find_with_uniqueness(args, false, false).await?;
            }
            "assert_state" => {
                let e = self.find(args, false).await?;
                let property = required(args, "property")?;
                if e.get(property).and_then(Value::as_bool).is_none() {
                    return Err(crate::outcome::FlowError::new(
                        crate::outcome::ErrorKind::Unsupported,
                        format!("{property} is unknown, not false"),
                    ));
                }
                if e[property] != args["expected"] {
                    return Err(format!("{property} does not match expected state").into());
                }
            }
            "assert_value" => self.assert_value(args, string(args, "expected")?).await?,
            _ => return Err(format!("unknown action {action}").into()),
        }
        Ok(None)
    }
}

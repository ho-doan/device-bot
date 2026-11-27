//! Element discovery, bounded waits, assertions, focus, and document scoping.
use crate::outcome::{ErrorKind, FlowError};

use super::arguments::{
    SCROLL, VIEWPORT, element_rectangle, gesture_arguments, query_arguments, rectangle,
    scroll_arguments, tap_arguments, type_arguments,
};
use super::artifacts::write;
use super::{BATCH, ElementProbeReport, ElementProbeSample, FlowRunner, QUERY, SWIPE, TAP};
use crate::{
    mcp::Result,
    schema::{self, Selector},
};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin, time::Duration};
use tokio::time::{Instant, sleep};

impl FlowRunner {
    /// Measure accessibility observation across bounded samples, without scrolling
    /// or changing the selected app. The enclosing step owns the hard deadline.
    pub(super) async fn probe_elements(&mut self, args: &Value) -> Result<ElementProbeReport> {
        let selector = schema::selector(args, &self.profile)?;
        let count = args["samples"].as_u64().unwrap_or(1);
        let interval = Duration::from_millis(args["interval_ms"].as_u64().unwrap_or(100));
        let mut samples = Vec::with_capacity(count as usize);
        let mut first_match_ms = None;
        let started = Instant::now();
        for index in 0..count {
            if index != 0 {
                sleep(interval).await;
            }
            let query_started = Instant::now();
            let count = self.query(&selector).await?.len();
            let observed = Instant::now();
            let elapsed_ms = (observed - started).as_millis() as u64;
            if count > 0 {
                first_match_ms.get_or_insert(elapsed_ms);
            }
            samples.push(ElementProbeSample {
                count,
                query_ms: (observed - query_started).as_millis() as u64,
                elapsed_ms,
            });
        }
        Ok(ElementProbeReport {
            first_match_ms,
            samples,
        })
    }

    pub(super) async fn assert_value(&mut self, args: &Value, expected: &str) -> Result<()> {
        let e = self.find(args, false).await?;
        // A native value is authoritative, even when its label matches the
        // expectation. Null is unknown; a label cannot supply that missing value.
        if let Some(value) = e.get("value") {
            return match value.as_str() {
                Some(value) if value == expected => Ok(()),
                Some(_) => Err("field does not expose expected value".into()),
                None => Err(FlowError::new(
                    ErrorKind::Unsupported,
                    "Unsupported: native field value is unknown",
                )),
            };
        }
        let exposed = ["text", "contentDescription"]
            .into_iter()
            .filter_map(|key| e.get(key).and_then(Value::as_str))
            .collect::<Vec<_>>();
        if exposed.is_empty() {
            return Err(FlowError::new(
                ErrorKind::Unsupported,
                "Unsupported: native field value is unknown",
            ));
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

    pub(super) async fn find(&mut self, args: &Value, tap: bool) -> Result<Value> {
        self.find_with_uniqueness(args, tap, true).await
    }

    pub(super) async fn find_with_uniqueness(
        &mut self,
        args: &Value,
        tap: bool,
        unique: bool,
    ) -> Result<Value> {
        self.find_matching(args, tap, tap, unique).await
    }

    pub(super) async fn find_gesture_target(&mut self, args: &Value) -> Result<Value> {
        // Some platforms expose long-only or double-click targets without a
        // single-click semantic action. Still require a unique enabled node.
        self.find_matching(args, false, true, true).await
    }

    async fn find_matching(
        &mut self,
        args: &Value,
        clickable: bool,
        enabled: bool,
        unique: bool,
    ) -> Result<Value> {
        let s = schema::selector(args, &self.profile)?;
        let desktop_points = self.device_id()?.starts_with("macos:");
        let deadline = Instant::now()
            + Duration::from_millis(
                args.get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(30_000),
            );
        tokio::time::timeout_at(deadline, async {
            let mut previous = Value::Null;
            let mut stable = 0;
            let mut attempts = 0;
            let mut viewport = None;
            let mut reveal_attempts = 0;
            loop {
                let mut found = self.query(&s).await?;
                if clickable {
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
                    return Err(
                        format!("ambiguous selector: {} matching elements", found.len()).into(),
                    );
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
                    let bounds = element_rectangle(e, desktop_points)?;
                    if (!enabled || e["enabled"] == true) && (!clickable || e["clickable"] == true)
                    {
                        let root_document = e.pointer("/webContext/framePath")
                            .and_then(Value::as_array).is_none_or(Vec::is_empty);
                        if clickable && s.scroll_into_view && root_document {
                            let device = self.device_id()?.to_owned();
                            let (width, height) = match viewport {
                                Some(size) => size,
                                None => {
                                    let result = self.call(VIEWPORT, json!({"deviceId":device})).await?;
                                    let metric = |key| result["structuredContent"][key].as_f64()
                                        .filter(|size| size.is_finite() && *size > 0.0)
                                        .ok_or("invalid browser viewport dimensions");
                                    let size = (metric("width")?, metric("height")?);
                                    viewport = Some(size);
                                    size
                                }
                            };
                            let center_x = (bounds.0 + bounds.2) / 2.0;
                            let center_y = (bounds.1 + bounds.3) / 2.0;
                            let dx = reveal_delta(center_x, width);
                            let dy = reveal_delta(center_y, height);
                            if dx != 0.0 || dy != 0.0 {
                                if reveal_attempts >= 6 {
                                    return Err("control remained outside the browser viewport after scrolling".into());
                                }
                                reveal_attempts += 1;
                                let scroll_x = center_x.clamp(width * 0.1, width * 0.9);
                                let scroll_y = center_y.clamp(height * 0.1, height * 0.9);
                                self.call(SCROLL, scroll_arguments(&device, scroll_x, scroll_y, dx, dy)).await?;
                                stable = 0;
                                previous = Value::Null;
                                sleep(Duration::from_millis(160)).await;
                                continue;
                            }
                        }
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
                sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|_| {
            "element missing, disabled, or unstable before bounded wait expired".to_owned()
        })?
    }

    pub(super) fn query<'a>(
        &'a mut self,
        s: &'a Selector,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>>> + Send + 'a>> {
        Box::pin(async move {
            let supports_source = self
                .tools
                .get(QUERY)
                .and_then(|schema| schema.pointer("/properties/source/enum"))
                .and_then(Value::as_array)
                .is_some_and(|values| {
                    values.contains(&json!("native")) && values.contains(&json!("flutter"))
                });
            if !supports_source {
                if s.source.as_deref().is_some_and(|v| v != "auto") {
                    return Err("MCP server does not support explicit element source".into());
                }
                return self.query_source(s, None).await;
            }
            if let Some(source) = s.source.as_deref().filter(|v| *v != "auto") {
                return self.query_source(s, Some(source)).await;
            }
            let native = self.query_source(s, Some("native")).await?;
            if !native.is_empty() {
                return Ok(native);
            }
            self.query_source(s, Some("flutter")).await
        })
    }

    pub(super) fn query_source<'a>(
        &'a mut self,
        s: &'a Selector,
        source: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>>> + Send + 'a>> {
        Box::pin(async move {
            let mut args = query_arguments(s, self.device_id()?);
            if let Some(source) = source {
                args["source"] = json!(source);
            }
            let result = self.call(QUERY, args).await?;
            let raw = result
                .pointer("/structuredContent/elements")
                .and_then(Value::as_array)
                .ok_or_else(|| FlowError::from("MCP elements missing").into_transport())?;
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
            if found.is_empty() {
                return Ok(found);
            }
            for (relation, exclude) in [(&s.row_with, false), (&s.exclude_row_with, true)] {
                if let Some(name) = relation {
                    let anchor = self
                        .profile
                        .selectors
                        .get(name)
                        .cloned()
                        .ok_or("unknown row anchor")?;
                    let anchors = self.query_source(&anchor, source).await?;
                    if anchors.is_empty() && s.allow_missing_row_anchor {
                        continue;
                    }
                    if anchors.is_empty() && source == Some("native") {
                        return Ok(vec![]);
                    }
                    if anchors.len() != 1 {
                        return Err("row anchor must match exactly one element".into());
                    }
                    let b = rectangle(&anchors[0])?;
                    let y = (b.1 + b.3) / 2.;
                    found.retain(|e| rectangle(e).is_ok_and(|b| (y >= b.1 && y <= b.3) != exclude));
                }
            }
            if let Some(position) = &s.position {
                found = schema::position_index(position, found.len())?
                    .map(|index| vec![found[index].clone()])
                    .unwrap_or_default();
            }
            Ok(found)
        })
    }

    pub(super) async fn focus(&mut self, args: &Value) -> Result<Value> {
        let s = schema::selector(args, &self.profile)?;
        for _ in 0..3 {
            let e = self.find(args, true).await?;
            self.tap_element(&s, &e).await?;
            if !s.require_focus {
                self.remember_web_focus(&e)?;
                return Ok(e);
            }
            let focused = self.find(args, false).await?;
            if focused.get("focused").and_then(Value::as_bool) == Some(true) {
                self.remember_web_focus(&focused)?;
                return Ok(focused);
            }
        }
        Err("editor focus was not observed".into())
    }

    pub(super) fn remember_web_focus(&mut self, element: &Value) -> Result<()> {
        let device = self.device_id()?.to_owned();
        if let Some(context) = element.get("webContext").filter(|value| !value.is_null()) {
            self.focused_web_contexts.insert(device, context.clone());
        } else {
            self.focused_web_contexts.remove(&device);
        }
        Ok(())
    }

    pub(super) fn type_payload(&self, text: &str, args: &Value) -> Result<Value> {
        let device = self.device_id()?;
        let mut payload = type_arguments(device, text, args)?;
        if let Some(expected) = args.get("web_context") {
            let selector = Selector {
                web_context: Some(
                    serde_json::from_value(expected.clone()).map_err(|error| error.to_string())?,
                ),
                ..Default::default()
            };
            if let Some(context) = self.focused_web_contexts.get(device).filter(|context| {
                element_context_matches(&selector, &json!({"webContext":context}))
            }) {
                payload["webContext"] = context.clone();
            }
        }
        Ok(payload)
    }

    pub(super) async fn tap_element(&mut self, s: &Selector, e: &Value) -> Result<()> {
        if s.native_tap
            && e.get("webContext")
                .is_some_and(|context| !context.is_null())
        {
            return Err("native_tap cannot use DOM viewport coordinates".into());
        }
        if s.native_tap {
            let filter = s
                .filter
                .as_ref()
                .or(s.label.as_ref())
                .ok_or("native tap requires label/filter")?;
            self.call(BATCH,json!({"deviceId":self.device_id()?,"actions":[{"type":"tap_element","filter":filter}]})).await?;
        } else {
            let device = self.device_id()?;
            // A window to the left of or above the primary display has
            // negative desktop coordinates. The scoped driver validates hits.
            let b = element_rectangle(e, device.starts_with("macos:"))?;
            self.call(TAP, tap_arguments(device, e, b)).await?;
        }
        Ok(())
    }

    pub(super) async fn scroll(&mut self, s: &Selector, direction: &str) -> Result<()> {
        if !["up", "down"].contains(&direction) {
            return Err("scroll direction must be up or down".into());
        }
        let found = self.query(s).await?;
        if found.len() != 1 {
            return Err("scroll viewport must match exactly one element".into());
        }
        if let Some(context) = found[0].get("webContext").filter(|value| !value.is_null()) {
            self.call("mobile_scroll", json!({"deviceId":self.device_id()?,"x":0,"y":0,"deltaX":0,"deltaY":if direction == "down" { 300 } else { -300 },"webContext":context})).await?;
            return Ok(());
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
}

fn reveal_delta(center: f64, viewport: f64) -> f64 {
    if (0.0..viewport).contains(&center) {
        0.0
    } else {
        let limit = (viewport * 0.85).min(1_500.0);
        (center - viewport / 2.0).clamp(-limit, limit)
    }
}

fn element_context_matches(selector: &Selector, element: &Value) -> bool {
    let context = &element["webContext"];
    let frame_path = context["framePath"].as_array().map(Vec::len).unwrap_or(0);
    let scoped = frame_path > 0 || context["appId"].as_str().is_some_and(|id| !id.is_empty());
    let Some(expected) = selector
        .web_context
        .as_ref()
        .filter(|context| !context.is_empty())
    else {
        return !scoped;
    };
    if expected.app_id.is_some() && expected.app_id.as_deref() != context["appId"].as_str() {
        return false;
    }
    if let Some(page) = &expected.page {
        if page.url.is_some() && page.url.as_deref() != context["page"]["url"].as_str() {
            return false;
        }
        if page.title.is_some() && page.title.as_deref() != context["page"]["title"].as_str() {
            return false;
        }
    }
    let actual_frames = context["framePath"].as_array().map(Vec::len).unwrap_or(0);
    actual_frames == expected.frame_path.len()
        && expected
            .frame_path
            .iter()
            .enumerate()
            .all(|(index, frame)| {
                let actual = &context["framePath"][index];
                frame.css.as_ref().is_none_or(|css| actual["css"] == *css)
                    && frame
                        .name
                        .as_ref()
                        .is_none_or(|name| actual["name"] == *name)
                    && frame.url.as_ref().is_none_or(|url| actual["url"] == *url)
                    && frame
                        .position
                        .is_none_or(|position| actual["position"] == position)
            })
}

pub fn filter_elements(s: &Selector, raw: &[Value]) -> Vec<Value> {
    let mut found = raw
        .iter()
        .filter(|e| element_context_matches(s, e))
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

#[cfg(test)]
mod web_context_tests {
    use super::*;
    use crate::schema::{FlowWebContext, FlowWebFrame, Selector};
    use std::path::PathBuf;

    #[test]
    fn recorded_resource_id_narrows_the_server_query_before_result_limit() {
        let selector = Selector {
            resource_id: Some("tab-evidence".into()),
            ..Selector::default()
        };
        assert_eq!(
            query_arguments(&selector, "chrome:fixture")["filter"],
            "tab-evidence"
        );
        // A preferred ID permits a label fallback and must not narrow that away.
        let fallback = Selector {
            prefer_resource_id: Some("old-id".into()),
            label: Some("Evidence".into()),
            ..Selector::default()
        };
        assert_eq!(
            query_arguments(&fallback, "chrome:fixture")["filter"],
            "Evidence"
        );
    }

    #[test]
    fn replay_input_retains_the_document_selected_by_focus() {
        let mut runner = FlowRunner::new(None, PathBuf::from("unused"));
        runner.device = Some(json!({"id":"chrome:fixture"}));
        let context =
            json!({"framePath":[{"css":"iframe#checkout"}],"elementRef":"original-document:1"});
        let element = json!({"webContext":context});
        runner.remember_web_focus(&element).unwrap();
        let payload = runner
            .type_payload(
                "paid",
                &json!({"web_context":{"frame_path":[{"css":"iframe#checkout"}]}}),
            )
            .unwrap();
        assert_eq!(payload["webContext"]["elementRef"], "original-document:1");
        assert_eq!(
            tap_arguments("chrome:fixture", &element, (1., 2., 3., 4.))["webContext"],
            context
        );
        let another = runner
            .type_payload(
                "other",
                &json!({"web_context":{"frame_path":[{"css":"iframe#other"}]}}),
            )
            .unwrap();
        assert!(another["webContext"].get("elementRef").is_none());
    }

    #[test]
    fn unscoped_selectors_stay_on_the_root_document() {
        let framed = json!({
            "className": "button",
            "text": "Pay",
            "webContext": {"framePath": [{"css": "iframe#checkout"}]}
        });
        let root = json!({"className": "button", "text": "Home"});
        let unscoped = filter_elements(&Selector::default(), &[framed.clone(), root.clone()]);
        assert_eq!(unscoped, vec![root.clone()]);
        let scoped = Selector {
            web_context: Some(FlowWebContext {
                frame_path: vec![FlowWebFrame {
                    css: Some("iframe#checkout".into()),
                    ..FlowWebFrame::default()
                }],
                ..FlowWebContext::default()
            }),
            ..Selector::default()
        };
        assert_eq!(
            filter_elements(&scoped, &[framed.clone(), root]),
            vec![framed]
        );
    }
}

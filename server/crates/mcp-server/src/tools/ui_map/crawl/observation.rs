//! Reads, normalizes, and settles snapshots of the selected target.

use super::super::Control;
use super::super::viewport;
use super::scope::parse_web;
use super::{Engine, LiveControl, Observation, Options, Scope, clipped};
use robot_core::{ElementQuery, ElementType, UiElement, prepare_elements};
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::time::Duration;
use tokio::time::Instant;

impl Engine<'_> {
    pub(super) async fn native_tree(&self) -> Result<(Vec<UiElement>, bool), String> {
        let mut query = ElementQuery {
            include_frames: matches!(self.scope, Scope::Web { .. }),
            exclude_system_keyboard: self.options.exclude_system_keyboard,
            max_results: Some(450),
            ..Default::default()
        };
        let mut root_only = false;
        let elements = match self
            .timing
            .measure(
                "ui_tree",
                self.robot.list_elements_query(&self.device, query.clone()),
            )
            .await
        {
            Ok(elements) => elements,
            Err(_) if query.include_frames => {
                query.include_frames = false;
                root_only = true;
                self.timing
                    .measure(
                        "ui_tree",
                        self.robot.list_elements_query(&self.device, query),
                    )
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        Ok((elements, root_only))
    }

    pub(super) async fn flutter_tree(&self) -> Result<Vec<UiElement>, String> {
        self.timing
            .measure("ui_tree", self.robot.list_flutter_elements(&self.device))
            .await
            .map_err(|error| error.to_string())
    }

    pub(super) async fn observe(&self) -> Result<Observation, String> {
        let location = self.context().await?;
        let (mut elements, root_only) = if self.flutter.get() == Some(&true) {
            let mut elements = self.flutter_tree().await?;
            if elements.is_empty() {
                // A bounded bridge snapshot can miss a frame. Retry the same
                // source once without accepting an empty screen as a state.
                tokio::time::sleep(Duration::from_millis(140)).await;
                self.check()?;
                elements = self.flutter_tree().await?;
            }
            if elements.is_empty() {
                return Err("The selected macOS window stopped exposing its Flutter tree.".into());
            }
            (elements, false)
        } else {
            let (mut elements, root_only) = self.native_tree().await?;
            if self.device.starts_with("macos:") && self.flutter.get().is_none() {
                prepare_elements(&mut elements);
                let native_has_controls = elements.iter().any(|element| {
                    live_control(element, self.options).is_some_and(|row| !row.lookup.is_empty())
                });
                if !native_has_controls
                    && let Ok(flutter) = self.flutter_tree().await
                    && !flutter.is_empty()
                {
                    let _ = self.flutter.set(true);
                    elements = flutter;
                } else {
                    let _ = self.flutter.set(false);
                }
            }
            (elements, root_only)
        };
        prepare_elements(&mut elements);
        let viewport = if self.device.starts_with("macos:") {
            viewport::inspect(&elements, self.flutter.get() == Some(&true))
        } else if self.scope.is_android() {
            viewport::inspect_android(&elements)
        } else if self.scope.is_scoped_ios() || matches!(self.scope, Scope::IosNative) {
            viewport::inspect_ios(&elements)
        } else {
            Ok(None)
        };
        let title = elements
            .iter()
            .find(|e| {
                matches!(
                    e.class_name.to_ascii_lowercase().as_str(),
                    "h1" | "h2" | "navigationbar" | "xcuielementtypenavigationbar"
                )
            })
            .and_then(|e| candidate(&e.content_description).or_else(|| candidate(&e.text)));
        let mut dialogs: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.class_name.as_str(), "dialog" | "alertdialog"))
            .map(|e| candidate(&e.content_description).or_else(|| candidate(&e.text)))
            .collect();
        dialogs.sort_unstable();
        let mut controls: Vec<_> = elements
            .iter()
            .filter_map(|e| live_control(e, self.options))
            .collect();
        let limited = controls.len() > 160 || elements.len() >= 450;
        let mut counts = HashMap::new();
        for row in &controls {
            *counts.entry(row.key.clone()).or_insert(0usize) += 1;
        }
        for (index, row) in controls.iter_mut().enumerate() {
            row.control.id = format!("c{}", index + 1);
            if counts[&row.key] != 1 {
                row.action = None;
                row.control.status = "skipped".into();
                row.control.note = Some("Selector is ambiguous in this screen.".into());
            }
            if let Scope::Web { start } = &self.scope
                && row.element.web_context.as_ref().is_some_and(|context| {
                    context
                        .frame_path
                        .iter()
                        .filter_map(|frame| frame.url.as_deref())
                        .any(|url| parse_web(url).is_ok_and(|url| url.origin() != start.origin()))
                })
            {
                row.action = None;
                row.control.status = "skipped".into();
                row.control.note =
                    Some("Control is in a frame outside the starting origin.".into());
            }
        }
        let mut identities: Vec<_> = controls
            .iter()
            .map(|c| {
                (
                    &c.key,
                    (c.control.kind != "input").then_some(&c.control.label),
                    c.element.enabled,
                    c.element.selected,
                    c.element.checked,
                    c.control.kind == "input"
                        && c.lookup.iter().any(|key| {
                            self.options.inputs.get(key).is_some_and(|expected| {
                                c.element.value.as_ref().or(c.element.text.as_ref())
                                    == Some(expected)
                            })
                        }),
                    c.action,
                )
            })
            .collect();
        identities.sort_unstable();
        identities.dedup();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        location.hash(&mut hasher);
        title.hash(&mut hasher);
        dialogs.hash(&mut hasher);
        let mut control_hasher = hasher.clone();
        let mut selection_independent = identities
            .iter()
            .map(|(key, label, enabled, selected, checked, filled, action)| {
                (
                    key,
                    label,
                    enabled,
                    selected.is_some(),
                    checked.is_some(),
                    filled,
                    action,
                )
            })
            .collect::<Vec<_>>();
        selection_independent.sort_unstable();
        selection_independent.dedup();
        selection_independent.hash(&mut control_hasher);
        let control_signature = control_hasher.finish();
        identities.hash(&mut hasher);
        let fingerprint = hasher.finish();
        controls.truncate(160);
        let title = title
            .or_else(|| {
                location
                    .as_deref()
                    .and_then(|l| l.rsplit('/').find(|part| !part.is_empty()))
                    .map(|s| clipped(s, 120))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "UI screen".into());
        Ok(Observation {
            fingerprint,
            control_signature,
            title,
            location,
            controls,
            limited,
            root_only,
            ambiguous_viewport: viewport.is_err(),
            native_viewport: viewport.ok().flatten(),
        })
    }

    pub(super) async fn settled(&self) -> Result<Observation, String> {
        tokio::time::sleep(Duration::from_millis(self.options.delay())).await;
        self.settled_after_wait().await
    }

    pub(super) async fn settled_after_wait(&self) -> Result<Observation, String> {
        let mut last = self.observe().await?;
        // Desktop route transitions can keep the same control signature while
        // animating the clipped viewport into place.
        let retries = if self.device.starts_with("macos:") {
            7
        } else {
            3
        };
        for attempt in 0..retries {
            tokio::time::sleep(Duration::from_millis(140)).await;
            let next = self.observe().await?;
            if last.fingerprint == next.fingerprint
                && match (&last.native_viewport, &next.native_viewport) {
                    (None, None) => {
                        // An outgoing Flutter route can briefly expose a second
                        // scroll container. A genuinely ambiguous screen still
                        // gets its visible controls and a coverage note.
                        !last.ambiguous_viewport && !next.ambiguous_viewport
                            || (last.ambiguous_viewport
                                && next.ambiguous_viewport
                                && attempt + 1 == retries)
                    }
                    (Some(last), Some(next)) => last.same_container(next),
                    _ => false,
                }
            {
                return Ok(next);
            }
            last = next;
        }
        Err("The UI did not settle to a repeatable screen signature.".into())
    }

    pub(super) async fn settled_entry(&self) -> Result<Observation, String> {
        let mut observation = self.settled().await?;
        if matches!(self.scope, Scope::Web { .. }) || self.scope.is_mobile() {
            // A fresh document or app can report its location before publishing controls.
            let ready_by = Instant::now() + Duration::from_secs(4);
            while observation.controls.is_empty()
                && observation.native_viewport.is_none()
                && Instant::now() < ready_by
            {
                self.check()?;
                observation = self.settled().await?;
            }
        }
        Ok(observation)
    }

    pub(super) async fn observe_known(&self, accepted: &[u64]) -> Result<Observation, String> {
        if !self.scope.is_mobile() {
            return self.settled().await;
        }
        tokio::time::sleep(Duration::from_millis(self.options.delay())).await;
        let mut here = self.observe().await?;
        for _ in 0..3 {
            if accepted.contains(&here.fingerprint) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(140)).await;
            here = self.observe().await?;
        }
        Ok(here)
    }

    pub(super) async fn wait_for_mobile_relaunch(
        &self,
        accepted: &[u64],
    ) -> Result<Observation, String> {
        tokio::time::sleep(Duration::from_millis(self.options.delay())).await;
        let ready_by = Instant::now() + Duration::from_secs(10);
        let mut remaining = 3;
        loop {
            self.check()?;
            match self.observe().await {
                Ok(here) if accepted.is_empty() => {
                    if !here.controls.is_empty() || here.native_viewport.is_some() {
                        return Ok(here);
                    }
                    if Instant::now() >= ready_by {
                        return Err("The relaunched fixture did not expose its UI.".into());
                    }
                    tokio::time::sleep(Duration::from_millis(140)).await;
                }
                Ok(here) if accepted.contains(&here.fingerprint) || remaining == 0 => {
                    return Ok(here);
                }
                Ok(_) => {
                    remaining -= 1;
                    tokio::time::sleep(Duration::from_millis(140)).await;
                }
                Err(error) if Instant::now() >= ready_by => {
                    return Err(format!("Could not verify the relaunched fixture: {error}"));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }

    pub(super) async fn wait_for_window_entry(
        &self,
        root: u64,
        expected: u64,
    ) -> Result<Observation, String> {
        let ready_by = Instant::now() + Duration::from_secs(5);
        loop {
            self.check()?;
            match self.settled().await {
                Ok(here) if [root, expected].contains(&here.fingerprint) => return Ok(here),
                Ok(here) if Instant::now() >= ready_by => return Ok(here),
                Err(error) if Instant::now() >= ready_by => {
                    return Err(format!("Could not inspect the relaunched fixture: {error}"));
                }
                _ => {}
            }
        }
    }
}

fn candidate(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(|s| s.lines().map(str::trim).find(|s| !s.is_empty()))
        .map(|s| clipped(s, 160))
}

pub(in crate::tools::ui_map) fn is_xctest_node_id(id: &str) -> bool {
    id.strip_prefix("ios-").is_some_and(|part| {
        (part.len() == 36 && uuid::Uuid::parse_str(part).is_ok())
            || part.split_once('-').is_some_and(|(generation, index)| {
                [generation, index]
                    .into_iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            })
    })
}

fn live_control(element: &UiElement, opt: &Options) -> Option<LiveControl> {
    let resource_id = candidate(&element.resource_id);
    let xctest_node = resource_id.as_deref().is_some_and(is_xctest_node_id);
    let kind = match element.element_type {
        Some(ElementType::TextField) => "input",
        Some(ElementType::Button) => "button",
        Some(ElementType::Link) => "link",
        // XCTest reports whether elements can be hit, including static text.
        Some(ElementType::StaticText | ElementType::Image) if xctest_node => return None,
        _ if element.clickable => "control",
        _ => return None,
    };
    let b = element.bounds;
    if b.right <= b.left || b.bottom <= b.top {
        return None;
    }
    let resource_id =
        resource_id.filter(|id| !xctest_node && !id.starts_with("flt-semantic-node-"));
    // The text of a native editor may be its value. Never use it as a label.
    let exposed_label = candidate(&element.content_description).or_else(|| {
        (kind != "input")
            .then(|| candidate(&element.text))
            .flatten()
    });
    let label = exposed_label
        .clone()
        .or_else(|| resource_id.clone())
        .unwrap_or_else(|| format!("Unlabeled {kind}"));
    let lookup: Vec<_> = [resource_id.clone(), exposed_label]
        .into_iter()
        .flatten()
        .collect();
    let mut context = element.web_context.clone();
    if let Some(ctx) = &mut context {
        ctx.element_ref = None;
    }
    let key = format!(
        "{kind}|{}|{}",
        lookup.first().map(String::as_str).unwrap_or(&label),
        serde_json::to_string(&context).unwrap_or_default()
    );
    let mut row = LiveControl {
        control: Control {
            id: String::new(),
            label,
            resource_id,
            kind: kind.into(),
            status: "skipped".into(),
            note: None,
            evidence: None,
            initially_offscreen: None,
        },
        key,
        lookup,
        element: element.clone(),
        action: None,
        reveal: None,
    };
    let note = if !element.enabled {
        "Disabled"
    } else if row.lookup.is_empty() {
        "No stable label or resource ID"
    } else if kind == "input" {
        if row.lookup.iter().any(|key| opt.inputs.contains_key(key)) {
            row.action = Some("input");
            ""
        } else {
            "No synthetic input configured"
        }
    } else if element.clickable
        && (opt.all_buttons
            || row
                .lookup
                .iter()
                .any(|key| opt.allowed_taps.contains(key) || opt.fixture_controls.contains(key)))
    {
        row.action = Some("tap");
        ""
    } else {
        "Not selected for this scan"
    };
    if row.action.is_some() {
        row.control.status = "pending".into();
    } else {
        row.control.note = Some(note.into());
    }
    Some(row)
}

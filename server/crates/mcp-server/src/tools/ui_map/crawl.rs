//! Bounded exploration over Robot input and accessibility, without app-specific scripts.
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use robot_core::{
    DeviceState, ElementQuery, ElementType, Platform, Point, Robot, UiElement, prepare_elements,
};
use serde::Deserialize;
use tokio::time::Instant;
use url::Url;

use super::{Control, Screen, Transition, UiMap, save_map};

pub(super) const MAX_SCREENS: usize = 250;
pub(super) const MAX_ACTIONS: usize = 600;
pub(super) const MAX_DEPTH: usize = 16;
pub(super) const MAX_TIME_SECONDS: u64 = 1_800;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub device_id: String,
    #[serde(default)]
    pub all_buttons: bool,
    #[serde(default)]
    pub allowed_taps: Vec<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    pub back_control: Option<String>,
    pub max_screens: Option<usize>,
    pub max_actions: Option<usize>,
    pub max_depth: Option<usize>,
    pub settle_ms: Option<u64>,
    pub max_time_seconds: Option<u64>,
}

impl Options {
    pub fn validate(&self) -> Result<(), String> {
        let key_ok = |key: &str| !key.trim().is_empty() && key.len() <= 180;
        if self.device_id.is_empty()
            || self.device_id.len() > 270
            || self.allowed_taps.len() > 150
            || !self.allowed_taps.iter().all(|s| key_ok(s))
            || self.inputs.len() > 30
            || self.inputs.iter().any(|(k, v)| {
                !key_ok(k) || v.is_empty() || v.len() > 300 || v.chars().any(char::is_control)
            })
            || self.back_control.as_ref().is_some_and(|s| !key_ok(s))
            || !(1..=MAX_SCREENS).contains(&self.screens())
            || !(1..=MAX_ACTIONS).contains(&self.actions())
            || !(1..=MAX_DEPTH).contains(&self.depth())
            || !(100..=2500).contains(&self.delay())
            || !(30..=MAX_TIME_SECONDS).contains(&self.time_seconds())
        {
            return Err("Invalid scan options. Limits: screens 1..250, actions 1..600, depth 1..16, settleMs 100..2500, maxTimeSeconds 30..1800. Use exact nonempty labels or resource IDs and synthetic single-line input.".into());
        }
        Ok(())
    }

    fn screens(&self) -> usize {
        self.max_screens.unwrap_or(24)
    }
    fn actions(&self) -> usize {
        self.max_actions.unwrap_or(45)
    }
    fn depth(&self) -> usize {
        self.max_depth.unwrap_or(5)
    }
    fn delay(&self) -> u64 {
        self.settle_ms.unwrap_or(350)
    }
    pub(super) fn time_seconds(&self) -> u64 {
        self.max_time_seconds.unwrap_or(480)
    }
}

#[derive(Clone)]
struct LiveControl {
    control: Control,
    key: String,
    lookup: Vec<String>,
    element: UiElement,
    action: Option<&'static str>,
}

struct Observation {
    fingerprint: u64,
    title: String,
    location: Option<String>,
    controls: Vec<LiveControl>,
    limited: bool,
    root_only: bool,
}

struct PathStep {
    key: String,
    action: &'static str,
    before: u64,
    after: u64,
}

enum Scope {
    Web { start: Url },
    Android { app: String },
    Native,
}

struct Engine<'a> {
    robot: &'a dyn Robot,
    options: &'a Options,
    scope: Scope,
    stop: &'a AtomicBool,
    deadline: Instant,
    input_budget: usize,
}

impl Engine<'_> {
    fn check(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Scan stopped by the operator.".into());
        }
        if Instant::now() >= self.deadline || self.input_budget == 0 {
            return Err("Scan time or navigation replay budget reached.".into());
        }
        Ok(())
    }

    async fn context(&self) -> Result<Option<String>, String> {
        self.check()?;
        match &self.scope {
            Scope::Native => Ok(None),
            Scope::Android { app } => {
                let context = self
                    .robot
                    .stream_context(&self.options.device_id)
                    .await
                    .map_err(|e| e.to_string())?;
                if context["appId"].as_str() != Some(app) {
                    return Err("Scan stopped because the foreground Android app changed.".into());
                }
                Ok(Some(app.clone()))
            }
            Scope::Web { start } => {
                let ctx = self
                    .robot
                    .stream_context(&self.options.device_id)
                    .await
                    .map_err(|e| e.to_string())?;
                let here = parse_web(ctx["url"].as_str().unwrap_or_default())?;
                if here.origin() != start.origin() {
                    return Err("Scan stopped because the tab left the starting origin.".into());
                }
                Ok(Some(web_location(&here)))
            }
        }
    }

    async fn observe(&self) -> Result<Observation, String> {
        let location = self.context().await?;
        let mut query = ElementQuery {
            include_frames: matches!(self.scope, Scope::Web { .. }),
            max_results: Some(450),
            ..Default::default()
        };
        let mut root_only = false;
        let mut elements = match self
            .robot
            .list_elements_query(&self.options.device_id, query.clone())
            .await
        {
            Ok(elements) => elements,
            Err(_) if query.include_frames => {
                query.include_frames = false;
                root_only = true;
                self.robot
                    .list_elements_query(&self.options.device_id, query)
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        prepare_elements(&mut elements);
        let title = elements
            .iter()
            .find(|e| {
                matches!(
                    e.class_name.to_ascii_lowercase().as_str(),
                    "h1" | "h2" | "navigationbar" | "xcuielementtypenavigationbar"
                )
            })
            .and_then(|e| candidate(&e.content_description).or_else(|| candidate(&e.text)));
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
                    c.element.enabled,
                    c.element.selected,
                    c.element.checked,
                )
            })
            .collect();
        identities.sort_unstable();
        identities.dedup();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        location.hash(&mut hasher);
        title.hash(&mut hasher);
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
            title,
            location,
            controls,
            limited,
            root_only,
        })
    }

    async fn settled(&self) -> Result<Observation, String> {
        tokio::time::sleep(Duration::from_millis(self.options.delay())).await;
        let mut last = self.observe().await?;
        for _ in 0..3 {
            tokio::time::sleep(Duration::from_millis(140)).await;
            let next = self.observe().await?;
            if last.fingerprint == next.fingerprint {
                return Ok(next);
            }
            last = next;
        }
        Err("The UI did not settle to a repeatable screen signature.".into())
    }

    async fn perform(&mut self, row: &LiveControl, action: &str) -> Result<(), String> {
        self.check()?;
        self.input_budget -= 1;
        let device = &self.options.device_id;
        let b = &row.element.bounds;
        let point = Point {
            x: (f64::from(b.left) + f64::from(b.right)) / 2.,
            y: (f64::from(b.top) + f64::from(b.bottom)) / 2.,
        };
        self.robot
            .tap_in_context(device, point, row.element.web_context.as_ref())
            .await
            .map_err(|_| "Driver could not tap the inspected control.".to_string())?;
        if action == "input" {
            let latest = self.settled().await?;
            let editor = latest
                .controls
                .iter()
                .find(|c| c.key == row.key && c.element.focused == Some(true))
                .ok_or("The configured editor did not report focus. No text was sent.")?;
            let context = editor.element.web_context.as_ref();
            let text = row
                .lookup
                .iter()
                .find_map(|key| self.options.inputs.get(key))
                .ok_or("No synthetic input is configured for this editor.")?;
            self.check()?;
            self.robot
                .clear_text_in_context(device, context)
                .await
                .map_err(|_| "Could not clear the editor. No text was sent.")?;
            self.check()?;
            self.robot
                .type_text_in_context(device, text, context)
                .await
                .map_err(|_| "Driver could not enter the configured text.".to_owned())?;
        }
        Ok(())
    }

    async fn restore(
        &mut self,
        expected: u64,
        root: u64,
        path: &[std::sync::Arc<PathStep>],
    ) -> Result<Observation, String> {
        let mut here = self.observe().await?;
        if here.fingerprint == expected {
            return Ok(here);
        }

        if matches!(self.scope, Scope::Web { .. }) {
            let Scope::Web { start } = &self.scope else {
                unreachable!()
            };
            self.check()?;
            self.input_budget -= 1;
            self.robot
                .open_url(&self.options.device_id, start.as_str(), None)
                .await
                .map_err(|_| "Could not reopen the entry URL.".to_owned())?;
            here = self.settled().await?;
        } else {
            let mut visited = HashSet::new();
            while here.fingerprint != root
                && here.fingerprint != expected
                && visited.insert(here.fingerprint)
                && visited.len() <= self.options.depth() + 1
            {
                let mut back = here.controls.iter().filter(|c| {
                    c.element.enabled
                        && c.control.kind != "input"
                        && c.control.note.as_deref()
                            != Some("Selector is ambiguous in this screen.")
                        && !blocked(c)
                        && if let Some(key) = &self.options.back_control {
                            c.lookup.contains(key)
                        } else {
                            c.lookup.iter().any(|s| {
                                matches!(
                                    s.to_lowercase().as_str(),
                                    "back"
                                        | "navigate up"
                                        | "close"
                                        | "cancel"
                                        | "quay lại"
                                        | "đóng"
                                        | "hủy"
                                )
                            })
                        }
                });
                let Some(row) = back.next() else { break };
                if back.next().is_some() {
                    break;
                }
                self.perform(row, "tap").await?;
                here = self.settled().await?;
            }
        }
        if here.fingerprint == expected {
            return Ok(here);
        }
        if here.fingerprint != root {
            return Err("Cannot restore the entry screen safely. The map is partial.".into());
        }
        for step in path {
            if here.fingerprint != step.before {
                return Err("The recorded route now leads to a different screen.".into());
            }
            let row = here
                .controls
                .iter()
                .find(|c| c.key == step.key && c.action == Some(step.action))
                .ok_or("A control on the recorded route is no longer uniquely available.")?;
            self.perform(row, step.action).await?;
            here = self.settled().await?;
            if here.fingerprint != step.after {
                return Err("The recorded route no longer reproduces the observed screen.".into());
            }
        }
        if here.fingerprint != expected {
            return Err("Could not restore the intended screen.".into());
        }
        Ok(here)
    }
}

fn clipped(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn candidate(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(|s| s.lines().map(str::trim).find(|s| !s.is_empty()))
        .map(|s| clipped(s, 160))
}

fn live_control(element: &UiElement, opt: &Options) -> Option<LiveControl> {
    let kind = match element.element_type {
        Some(ElementType::TextField) => "input",
        Some(ElementType::Button) => "button",
        Some(ElementType::Link) => "link",
        _ if element.clickable => "control",
        _ => return None,
    };
    let b = element.bounds;
    if b.right <= b.left || b.bottom <= b.top {
        return None;
    }
    let resource_id = candidate(&element.resource_id);
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
        },
        key,
        lookup,
        element: element.clone(),
        action: None,
    };
    let note = if !element.enabled {
        "Disabled"
    } else if row.lookup.is_empty() {
        "No stable label or resource ID"
    } else if blocked(&row) {
        "Sensitive or state-changing control"
    } else if kind == "input" {
        if row.lookup.iter().any(|key| opt.inputs.contains_key(key)) {
            row.action = Some("input");
            ""
        } else {
            "No synthetic input configured"
        }
    } else if element.clickable
        && (opt.all_buttons || row.lookup.iter().any(|key| opt.allowed_taps.contains(key)))
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

fn blocked(row: &LiveControl) -> bool {
    let words = format!("{} {}", row.lookup.join(" "), row.element.class_name).to_lowercase();
    [
        "password",
        "passcode",
        "securetext",
        "one-time",
        "authorization",
        "sign out",
        "log out",
        "logout",
        "delete",
        "destroy",
        "purchase",
        "checkout",
        "payment",
        "send message",
        "remove account",
        "submit",
        "confirm",
        "erase",
        "đăng xuất",
        "mật khẩu",
        "xóa",
        "thanh toán",
        "gửi tin",
        "đặt hàng",
    ]
    .into_iter()
    .any(|w| words.contains(w))
        || words
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| matches!(w, "send" | "save" | "pay" | "otp"))
}

fn parse_web(value: &str) -> Result<Url, String> {
    let url = Url::parse(value)
        .map_err(|_| "Open an HTTP(S) page in the selected Chrome tab first.".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("UI mapping requires an HTTP(S) tab URL without embedded credentials.".into());
    }
    Ok(url)
}

fn web_location(url: &Url) -> String {
    let mut location = format!("{}{}", url.origin().ascii_serialization(), url.path());
    if let Some(route) = url.fragment().filter(|f| f.starts_with('/')) {
        location.push('#');
        location.push_str(route.split('?').next().unwrap_or_default());
    }
    clipped(&location, 240)
}

async fn classify_scope(robot: &dyn Robot, opt: &Options) -> Result<Scope, String> {
    let device = &opt.device_id;
    if device.starts_with("chrome:") {
        let context = robot
            .stream_context(device)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(Scope::Web {
            start: parse_web(context["url"].as_str().unwrap_or_default())?,
        });
    }
    if ["windows:", "macos:"]
        .iter()
        .any(|prefix| device.starts_with(prefix))
    {
        if device.split(':').count() != 3 {
            return Err("Select a desktop process and window before collecting a map.".into());
        }
        return Ok(Scope::Native);
    }
    let devices = robot.list_devices().await.map_err(|e| e.to_string())?;
    let target = devices
        .iter()
        .find(|d| d.id == *device && d.state == DeviceState::Available)
        .ok_or("Selected device is unavailable.")?;
    match target.platform {
        Platform::Android => {
            let context = robot
                .stream_context(device)
                .await
                .map_err(|e| e.to_string())?;
            let app = context["appId"]
                .as_str()
                .ok_or("Open the Android app before mapping.")?;
            Ok(Scope::Android {
                app: app.to_owned(),
            })
        }
        Platform::Ios => Ok(Scope::Native),
        _ => Err("Select a specific Chrome tab or desktop window.".into()),
    }
}

async fn record(engine: &Engine<'_>, map: &mut UiMap, dir: &Path, obs: &Observation) -> String {
    let id = format!("s{}", map.screens.len() + 1);
    let screenshot = match tokio::time::timeout(
        Duration::from_secs(10),
        engine.robot.take_screenshot(&engine.options.device_id),
    )
    .await
    {
        Ok(Ok(png))
            if png.len() <= super::MAX_IMAGE_BYTES && png.starts_with(b"\x89PNG\r\n\x1a\n") =>
        {
            engine
                .observe()
                .await
                .is_ok_and(|current| current.fingerprint == obs.fingerprint)
                && tokio::fs::write(dir.join(format!("{id}.png")), png)
                    .await
                    .is_ok()
        }
        _ => false,
    };
    if obs.limited && !map.notes.iter().any(|n| n.starts_with("Control inventory")) {
        map.notes
            .push("Control inventory was capped on at least one screen.".into());
    }
    if obs.root_only && !map.notes.iter().any(|n| n.starts_with("Frame inspection")) {
        map.notes.push(
            "Frame inspection was unavailable. Only the root document was inventoried.".into(),
        );
    }
    map.screens.push(Screen {
        id: id.clone(),
        title: obs.title.clone(),
        location: obs.location.clone(),
        screenshot,
        controls: obs.controls.iter().map(|c| c.control.clone()).collect(),
    });
    id
}

fn mark(map: &mut UiMap, screen: &str, control: &str, status: &str, note: Option<String>) {
    if let Some(control) = map
        .screens
        .iter_mut()
        .find(|s| s.id == screen)
        .and_then(|s| s.controls.iter_mut().find(|c| c.id == control))
    {
        control.status = status.into();
        control.note = note;
    }
}

pub(super) async fn scan(
    robot: &dyn Robot,
    opt: &Options,
    stop: &AtomicBool,
    map: &mut UiMap,
    dir: &Path,
) -> Result<(), String> {
    let scope = classify_scope(robot, opt).await?;
    let mut engine = Engine {
        robot,
        options: opt,
        scope,
        stop,
        deadline: Instant::now() + Duration::from_secs(opt.time_seconds()),
        input_budget: opt.actions() * 8 + 20,
    };
    let root = engine.settled().await?;
    let root_id = record(&engine, map, dir, &root).await;
    let mut seen = HashMap::from([(root.fingerprint, root_id.clone())]);
    let mut queue = VecDeque::from([(
        root.fingerprint,
        root_id,
        Vec::<std::sync::Arc<PathStep>>::new(),
        root.controls,
    )]);
    let root_fp = root.fingerprint;
    save_map(dir, map).await?;
    'crawl: while let Some((fingerprint, from, path, inventory)) = queue.pop_front() {
        for original in inventory.iter().filter(|c| c.action.is_some()) {
            if path.len() >= opt.depth() {
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "skipped",
                    Some("Depth limit".into()),
                );
                continue;
            }
            if map.attempted >= opt.actions() {
                map.notes
                    .push("Action budget reached. Pending controls were not tried.".into());
                break 'crawl;
            }
            engine.check()?;
            let here = engine.restore(fingerprint, root_fp, &path).await?;
            let action = original.action.unwrap();
            let Some(row) = here
                .controls
                .iter()
                .find(|c| c.key == original.key && c.action == Some(action))
            else {
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "skipped",
                    Some("Control changed before input.".into()),
                );
                continue;
            };
            map.attempted += 1;
            if let Err(err) = engine.perform(row, action).await {
                mark(map, &from, &original.control.id, "failed", Some(err));
                save_map(dir, map).await?;
                continue;
            }
            let next = match engine.settled().await {
                Ok(next) => next,
                Err(error) => {
                    mark(
                        map,
                        &from,
                        &original.control.id,
                        "failed",
                        Some(error.clone()),
                    );
                    return Err(error);
                }
            };
            if next.fingerprint == fingerprint {
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "no_change",
                    Some("No new screen signature observed.".into()),
                );
            } else {
                let to = if let Some(to) = seen.get(&next.fingerprint) {
                    to.clone()
                } else if map.screens.len() < opt.screens() {
                    let to = record(&engine, map, dir, &next).await;
                    seen.insert(next.fingerprint, to.clone());
                    let mut child_path = path.clone();
                    child_path.push(std::sync::Arc::new(PathStep {
                        key: original.key.clone(),
                        action,
                        before: fingerprint,
                        after: next.fingerprint,
                    }));
                    queue.push_back((next.fingerprint, to.clone(), child_path, next.controls));
                    to
                } else {
                    mark(
                        map,
                        &from,
                        &original.control.id,
                        "skipped",
                        Some("Changed screen, but screen budget reached.".into()),
                    );
                    map.notes.push("Screen budget reached.".into());
                    break 'crawl;
                };
                map.transitions.push(Transition {
                    from: from.clone(),
                    to,
                    control: original.control.id.clone(),
                    label: original.control.label.clone(),
                    resource_id: original.control.resource_id.clone(),
                    action: action.into(),
                });
                mark(map, &from, &original.control.id, "observed", None);
            }
            save_map(dir, map).await?;
        }
    }
    Ok(())
}

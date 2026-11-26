//! Bounded exploration over Robot input and accessibility, without app-specific scripts.
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use robot_core::{
    DeviceState, ElementQuery, ElementType, Platform, Point, Robot, UiElement, prepare_elements,
};
use serde::Deserialize;
use tokio::time::Instant;
use url::Url;

use super::{Control, Screen, Transition, UiMap, replay, save_map};

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
    pub fixture_controls: Vec<String>,
    #[serde(default)]
    pub expand_selection_states: bool,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    pub back_control: Option<String>,
    pub reset_app_id: Option<String>,
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
            || self.fixture_controls.len() > 30
            || !self.fixture_controls.iter().all(|s| key_ok(s))
            || self.inputs.len() > 30
            || self.inputs.iter().any(|(k, v)| {
                !key_ok(k) || v.is_empty() || v.len() > 300 || v.chars().any(char::is_control)
            })
            || self.back_control.as_ref().is_some_and(|s| !key_ok(s))
            || self.reset_app_id.as_ref().is_some_and(|id| {
                !key_ok(id)
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
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
    control_signature: u64,
    title: String,
    location: Option<String>,
    controls: Vec<LiveControl>,
    limited: bool,
    root_only: bool,
}

impl LiveControl {
    fn center(&self) -> Point {
        let bounds = &self.element.bounds;
        Point {
            x: (f64::from(bounds.left) + f64::from(bounds.right)) / 2.0,
            y: (f64::from(bounds.top) + f64::from(bounds.bottom)) / 2.0,
        }
    }

    fn replay_action<'a>(
        &'a self,
        options: &'a Options,
        initially_offscreen: Option<bool>,
    ) -> replay::Action<'a> {
        let input = if self.action == Some("input") {
            self.lookup
                .iter()
                .find_map(|key| options.inputs.get(key))
                .map(String::as_str)
        } else {
            None
        };
        replay::Action {
            element: &self.element,
            label: &self.control.label,
            resource_id: self.control.resource_id.as_deref(),
            input,
            initially_offscreen,
        }
    }
}

struct PathStep {
    key: String,
    action: &'static str,
    before: u64,
    after: u64,
    initially_offscreen: Option<bool>,
}

type Navigation = HashMap<u64, Vec<Arc<PathStep>>>;

fn known_route(navigation: &Navigation, from: u64, to: u64) -> Option<Vec<Arc<PathStep>>> {
    let mut queue = VecDeque::from([from]);
    let mut previous = HashMap::from([(from, None)]);
    while let Some(screen) = queue.pop_front() {
        if screen == to {
            let mut route = Vec::new();
            let mut screen = to;
            while let Some(step) = previous[&screen] {
                let step: &Arc<PathStep> = step;
                route.push(Arc::clone(step));
                screen = step.before;
            }
            route.reverse();
            return Some(route);
        }
        for step in navigation.get(&screen).into_iter().flatten() {
            if let std::collections::hash_map::Entry::Vacant(entry) = previous.entry(step.after) {
                entry.insert(Some(step));
                queue.push_back(step.after);
            }
        }
    }
    None
}

#[derive(Clone)]
pub(super) enum Scope {
    Web { start: Url },
    Mobile { app: String, platform: Platform },
    Native,
}

impl Scope {
    pub(super) fn is_android(&self) -> bool {
        matches!(
            self,
            Self::Mobile {
                platform: Platform::Android,
                ..
            }
        )
    }

    pub(super) async fn location(
        &self,
        robot: &dyn Robot,
        device: &str,
    ) -> Result<Option<String>, String> {
        match self {
            Self::Native => Ok(None),
            Self::Mobile { app, platform } => {
                let context = robot
                    .stream_context(device)
                    .await
                    .map_err(|e| e.to_string())?;
                if context["appId"].as_str() != Some(app) {
                    return Err(format!(
                        "Scan stopped because the foreground {platform:?} app changed."
                    ));
                }
                Ok(Some(app.clone()))
            }
            Self::Web { start } => {
                let ctx = robot
                    .stream_context(device)
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
}

struct Engine<'a> {
    robot: &'a Arc<dyn Robot>,
    dir: &'a Path,
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
        if Instant::now() >= self.deadline {
            return Err("Scan time limit reached.".into());
        }
        if self.input_budget == 0 {
            return Err("Navigation replay budget reached.".into());
        }
        Ok(())
    }

    async fn context(&self) -> Result<Option<String>, String> {
        self.check()?;
        self.scope
            .location(&**self.robot, &self.options.device_id)
            .await
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

    async fn settled_entry(&self) -> Result<Observation, String> {
        let mut observation = self.settled().await?;
        if matches!(self.scope, Scope::Web { .. }) {
            // A fresh document can report its route before publishing controls.
            let ready_by = Instant::now() + Duration::from_secs(4);
            while observation.controls.is_empty() && Instant::now() < ready_by {
                self.check()?;
                observation = self.settled().await?;
            }
        }
        Ok(observation)
    }

    async fn observe_known(&self, accepted: &[u64]) -> Result<Observation, String> {
        if !self.scope.is_android() {
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

    async fn perform(&mut self, row: &LiveControl, action: &str) -> Result<(), String> {
        self.check()?;
        self.input_budget -= 1;
        let device = &self.options.device_id;
        self.robot
            .tap_in_context(device, row.center(), row.element.web_context.as_ref())
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
        path: &[Arc<PathStep>],
        navigation: &Navigation,
    ) -> Result<Observation, String> {
        let mut here = self.observe().await?;
        if here.fingerprint == expected {
            return Ok(here);
        }

        if (matches!(self.scope, Scope::Web { .. }) || self.scope.is_android())
            && let Some(route) = known_route(navigation, here.fingerprint, expected)
                .filter(|route| route.len() <= path.len() + 1)
        {
            here = match self.replay_path(here, &route).await {
                Ok(restored) => return Ok(restored),
                Err(_) => self.observe().await?,
            };
            if here.fingerprint == expected {
                return Ok(here);
            }
        }

        if let Scope::Web { start } = &self.scope {
            self.check()?;
            self.input_budget -= 1;
            let device = &self.options.device_id;
            if self
                .robot
                .reset_web_document(device, start.as_str())
                .await
                .is_err()
            {
                self.context().await?;
                self.robot
                    .reset_web_document(device, start.as_str())
                    .await
                    .map_err(|error| format!("Could not reset the entry document: {error}"))?;
            }
            here = self.settled_entry().await?;
        } else if let Some(app) = &self.options.reset_app_id {
            self.check()?;
            self.input_budget -= 1;
            let device = &self.options.device_id;
            self.robot
                .terminate_app(device, app)
                .await
                .map_err(|error| format!("Could not stop the fixture app: {error}"))?;
            self.robot
                .launch_app(device, app)
                .await
                .map_err(|error| format!("Could not relaunch the fixture app: {error}"))?;
            here = self.observe_known(&[root, expected]).await?;
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
        let here = self.replay_path(here, path).await?;
        if here.fingerprint != expected {
            return Err("Could not restore the intended screen.".into());
        }
        Ok(here)
    }

    async fn replay_path(
        &mut self,
        mut here: Observation,
        path: &[Arc<PathStep>],
    ) -> Result<Observation, String> {
        for step in path {
            if here.fingerprint != step.before {
                return Err("The recorded route now leads to a different screen.".into());
            }
            let row = here
                .controls
                .iter()
                .find(|c| c.key == step.key && c.action == Some(step.action))
                .ok_or("A control on the recorded route is no longer uniquely available.")?;
            if step.initially_offscreen == Some(true) {
                self.check()?;
                self.input_budget -= 1;
                replay::restore(
                    self.robot,
                    &self.scope,
                    &self.options.device_id,
                    self.dir,
                    row.replay_action(self.options, step.initially_offscreen),
                    self.options.delay(),
                )
                .await?;
            } else {
                self.perform(row, step.action).await?;
            }
            here = self.observe_known(&[step.after]).await?;
            if here.fingerprint != step.after {
                return Err("The recorded route no longer reproduces the observed screen.".into());
            }
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
    let resource_id =
        candidate(&element.resource_id).filter(|id| !id.starts_with("flt-semantic-node-"));
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
    };
    let note = if !element.enabled {
        "Disabled"
    } else if row.lookup.is_empty() {
        "No stable label or resource ID"
    } else if blocked(&row)
        && !row
            .lookup
            .iter()
            .any(|key| opt.fixture_controls.contains(key))
    {
        "Sensitive or state-changing control"
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
        if opt.reset_app_id.is_some() {
            return Err(
                "Chrome restores the entry URL. resetAppId is only for mobile fixtures.".into(),
            );
        }
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
        if opt.reset_app_id.is_some() {
            return Err("resetAppId is only for mobile fixtures.".into());
        }
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
    match &target.platform {
        platform @ (Platform::Android | Platform::Ios)
            if *platform == Platform::Android || opt.reset_app_id.is_some() =>
        {
            let context = robot
                .stream_context(device)
                .await
                .map_err(|e| e.to_string())?;
            let app = context["appId"]
                .as_str()
                .filter(|app| !app.is_empty())
                .ok_or("Open the mobile app before mapping.")?;
            if opt
                .reset_app_id
                .as_deref()
                .is_some_and(|reset| reset != app)
            {
                return Err("resetAppId must match the currently open app.".into());
            }
            Ok(Scope::Mobile {
                app: app.to_owned(),
                platform: platform.clone(),
            })
        }
        Platform::Ios => Ok(Scope::Native),
        _ => Err("Select a specific Chrome tab or desktop window.".into()),
    }
}

async fn record(
    engine: &Engine<'_>,
    map: &mut UiMap,
    dir: &Path,
    obs: &mut Observation,
    same_controls_as: Option<&str>,
) -> String {
    let id = format!("s{}", map.screens.len() + 1);
    let viewport = if matches!(engine.scope, Scope::Web { .. }) {
        engine
            .robot
            .web_viewport(&engine.options.device_id)
            .await
            .ok()
            .map(|mut viewport| {
                for row in obs.controls.iter_mut().filter(|row| {
                    row.element
                        .web_context
                        .as_ref()
                        .is_none_or(|ctx| ctx.frame_path.is_empty())
                }) {
                    row.control.initially_offscreen = Some(!viewport.contains(row.center()));
                    viewport.content_width = viewport
                        .content_width
                        .max(f64::from(row.element.bounds.right));
                    viewport.content_height = viewport
                        .content_height
                        .max(f64::from(row.element.bounds.bottom));
                }
                viewport
            })
    } else {
        None
    };
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
        viewport,
        same_controls_as: same_controls_as.map(str::to_owned),
        controls: obs
            .controls
            .iter()
            .map(|c| {
                let mut control = c.control.clone();
                if let Some(screen) = same_controls_as
                    && control.status == "pending"
                {
                    control.status = "same_controls".into();
                    control.note = Some(format!("Same actionable controls as {screen}."));
                }
                control
            })
            .collect(),
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
    robot: &Arc<dyn Robot>,
    opt: &Options,
    stop: &AtomicBool,
    map: &mut UiMap,
    dir: &Path,
) -> Result<(), String> {
    let scope = classify_scope(&**robot, opt).await?;
    let mut engine = Engine {
        robot,
        dir,
        options: opt,
        scope,
        stop,
        deadline: Instant::now() + Duration::from_secs(opt.time_seconds()),
        input_budget: opt.actions() * (opt.depth() + 2) + 20,
    };
    let mut root = engine.settled_entry().await?;
    if matches!(engine.scope, Scope::Web { .. }) && root.controls.is_empty() {
        map.notes
            .push("No controls were found on the entry page after waiting for it to load.".into());
    }
    let root_id = record(&engine, map, dir, &mut root, None).await;
    let mut seen = HashMap::from([(root.fingerprint, root_id.clone())]);
    let mut control_signatures = HashMap::from([(root.control_signature, root_id.clone())]);
    let mut queue = VecDeque::from([(
        root.fingerprint,
        root_id,
        Vec::<Arc<PathStep>>::new(),
        root.controls,
    )]);
    let root_fp = root.fingerprint;
    let mut navigation = Navigation::new();
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
            let here = match engine
                .restore(fingerprint, root_fp, &path, &navigation)
                .await
            {
                Ok(here) => here,
                Err(error) if engine.scope.is_android() => {
                    // Preserve the app and scan budget guards when an individual route drifts.
                    engine.context().await?;
                    map.notes.push(format!("{from}: {error}"));
                    if let Some(screen) = map.screens.iter_mut().find(|screen| screen.id == from) {
                        for control in screen.controls.iter_mut().filter(|c| c.status == "pending")
                        {
                            control.status = "skipped".into();
                            control.note = Some("Could not restore this screen.".into());
                        }
                    }
                    save_map(dir, map).await?;
                    continue 'crawl;
                }
                Err(error) => return Err(error),
            };
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
            engine.input_budget -= 1;
            let name = format!("{from}-{}", original.control.id);
            let (evidence, error) = replay::run(
                robot,
                &engine.scope,
                &opt.device_id,
                dir,
                &name,
                row.replay_action(opt, original.control.initially_offscreen),
                opt.delay(),
            )
            .await?;
            if let Some(control) = map
                .screens
                .iter_mut()
                .find(|s| s.id == from)
                .and_then(|s| s.controls.iter_mut().find(|c| c.id == original.control.id))
            {
                control.evidence = Some(evidence);
            }
            if let Some(err) = error {
                mark(map, &from, &original.control.id, "failed", Some(err));
                save_map(dir, map).await?;
                engine.context().await?;
                continue;
            }
            let mut next = match engine.settled().await {
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
                let step = Arc::new(PathStep {
                    key: original.key.clone(),
                    action,
                    before: fingerprint,
                    after: next.fingerprint,
                    initially_offscreen: original.control.initially_offscreen,
                });
                let to = if let Some(to) = seen.get(&next.fingerprint) {
                    to.clone()
                } else if map.screens.len() < opt.screens() {
                    let representative = if opt.expand_selection_states {
                        None
                    } else {
                        control_signatures
                            .get(&next.control_signature)
                            .map(String::as_str)
                    };
                    let to = record(&engine, map, dir, &mut next, representative).await;
                    seen.insert(next.fingerprint, to.clone());
                    if representative.is_none() {
                        control_signatures.insert(next.control_signature, to.clone());
                        let mut child_path = path.clone();
                        child_path.push(Arc::clone(&step));
                        queue.push_back((next.fingerprint, to.clone(), child_path, next.controls));
                    }
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
                navigation.entry(fingerprint).or_default().push(step);
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

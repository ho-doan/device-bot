//! Bounded exploration over Robot input and accessibility, without app-specific scripts.
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    path::Path,
    sync::{
        Arc, OnceLock,
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

use super::{
    Control, Screen, Transition, UiMap, claim_recovered_device, replay, save_map,
    timing::TimingLog,
    viewport::{self, MAX_STEPS_PER_DIRECTION, Reveal, SCROLL_SETTLE_MS, Viewport},
};

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
    pub exclude_system_keyboard: bool,
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
            || self
                .max_time_seconds
                .is_some_and(|seconds| !(30..=MAX_TIME_SECONDS).contains(&seconds))
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
}

#[derive(Clone)]
struct LiveControl {
    control: Control,
    key: String,
    lookup: Vec<String>,
    element: UiElement,
    action: Option<&'static str>,
    reveal: Option<Reveal>,
}

struct Observation {
    fingerprint: u64,
    control_signature: u64,
    title: String,
    location: Option<String>,
    controls: Vec<LiveControl>,
    limited: bool,
    root_only: bool,
    native_viewport: Option<Viewport>,
    ambiguous_viewport: bool,
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
        flutter: bool,
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
            flutter,
            reveal: self.reveal.as_ref(),
        }
    }
}

struct PathStep {
    key: String,
    action: &'static str,
    before: u64,
    after: u64,
    initially_offscreen: Option<bool>,
    revealed_control: Option<LiveControl>,
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
    WindowFixture,
    IosNative,
    Native,
}

impl Scope {
    fn is_mobile(&self) -> bool {
        matches!(self, Self::Mobile { .. })
    }

    fn is_android(&self) -> bool {
        matches!(
            self,
            Self::Mobile {
                platform: Platform::Android,
                ..
            }
        )
    }

    fn is_scoped_ios(&self) -> bool {
        matches!(
            self,
            Self::Mobile {
                platform: Platform::Ios,
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
            Self::Native | Self::IosNative | Self::WindowFixture => Ok(None),
            Self::Mobile { app, platform } => {
                let foreground = foreground_app(robot, device, platform).await?;
                if foreground.as_deref() != Some(app) {
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
    device: String,
    window_valid: bool,
    scope: Scope,
    stop: &'a AtomicBool,
    timing: &'a TimingLog,
    flutter: OnceLock<bool>,
    deadline: Option<Instant>,
    input_budget: usize,
    last_same_screen: Option<Observation>,
    recovery_boot_target: Option<String>,
}

impl Engine<'_> {
    fn check(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Scan stopped by the operator.".into());
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err("Scan time limit reached.".into());
        }
        if self.input_budget == 0 {
            return Err("Navigation replay budget reached.".into());
        }
        Ok(())
    }

    async fn context(&self) -> Result<Option<String>, String> {
        self.check()?;
        self.timing
            .measure(
                "foreground_check",
                self.scope.location(&**self.robot, &self.device),
            )
            .await
    }

    async fn recover_target(
        &mut self,
        map_id: &str,
        failure: &str,
    ) -> Result<Option<String>, String> {
        self.check()?;
        let Scope::Mobile { app, platform } = &self.scope else {
            return Ok(None);
        };
        if self.options.reset_app_id.as_deref() != Some(app) {
            return Ok(None);
        }
        let detail = match platform {
            Platform::Android => {
                let Some(boot_target) = &self.recovery_boot_target else {
                    return Ok(None);
                };
                let devices = self
                    .robot
                    .list_devices()
                    .await
                    .map_err(|error| error.to_string())?;
                if devices
                    .iter()
                    .any(|d| d.id == self.device && d.state == DeviceState::Available)
                {
                    if is_mobile_transport_error(failure) && self.context().await.is_ok() {
                        "ADB recovered without restarting the AVD".into()
                    } else {
                        return Ok(None);
                    }
                } else {
                    self.robot
                        .boot_device(boot_target)
                        .await
                        .map_err(|error| format!("Boot/reconnect {boot_target}: {error}"))?;
                    let devices =
                        self.robot.list_devices().await.map_err(|error| {
                            format!("Discover {boot_target} after boot: {error}")
                        })?;
                    let mut recovered = None;
                    for device in devices.into_iter().filter(|d| {
                        d.platform == Platform::Android && d.state == DeviceState::Available
                    }) {
                        if self
                            .robot
                            .recovery_boot_target(&device.id)
                            .await
                            .ok()
                            .flatten()
                            .as_ref()
                            == Some(boot_target)
                        {
                            if recovered.replace(device.id).is_some() {
                                return Err(format!("More than one device matches {boot_target}."));
                            }
                        }
                    }
                    let device =
                        recovered.ok_or_else(|| format!("{boot_target} did not return to ADB."))?;
                    self.check()?;
                    claim_recovered_device(map_id, &device)?;
                    // Do not reset the app if transport came back with it still in scope.
                    let needs_launch = self.scope.location(&**self.robot, &device).await.is_err();
                    if needs_launch {
                        self.robot
                            .launch_app(&device, app)
                            .await
                            .map_err(|error| format!("Launch {app} on {boot_target}: {error}"))?;
                    }
                    self.device = device;
                    if needs_launch {
                        // An accepted launch is not proof that Android has
                        // finished showing the fixture after a cold boot.
                        self.wait_for_mobile_relaunch(&[]).await?;
                    }
                    format!("ADB reconnected to {boot_target} on {}", self.device)
                }
            }
            Platform::Ios => {
                let status = self
                    .robot
                    .check_device_health(&self.device)
                    .await
                    .map_err(|error| error.to_string())?;
                if status.ready
                    && status.target_app_id.as_deref() == Some(app)
                    && is_mobile_transport_error(failure)
                {
                    "XCTest bridge recovered without restarting the runner".into()
                } else if matches!(status.state.as_str(), "disconnected" | "bridge_unreachable") {
                    let recovered = self
                        .robot
                        .reboot_helper(&self.device, Some(app))
                        .await
                        .map_err(|error| {
                            format!(
                                "{}; restarting the XCTest runner failed: {error}",
                                status.detail
                            )
                        })?;
                    if !recovered.ready || recovered.target_app_id.as_deref() != Some(app) {
                        return Err(format!(
                            "{}; XCTest runner is still unavailable: {}",
                            status.detail, recovered.detail
                        ));
                    }
                    format!(
                        "Restarted the XCTest runner on the same iOS device after {}",
                        status.detail
                    )
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };
        self.context().await?;
        self.last_same_screen = None;
        Ok(Some(detail))
    }

    async fn recover_at(
        &mut self,
        map: &mut UiMap,
        screen: &str,
        failure: &str,
    ) -> Result<bool, String> {
        // Operator stop or an explicitly configured scan limit is not a
        // target disconnect. Preserve that reason without a recovery note.
        self.check()?;
        let recovery = self
            .timing
            .measure("device_recovery", self.recover_target(&map.id, failure))
            .await;
        match recovery {
            Ok(Some(detail)) => {
                map.device_id.clone_from(&self.device);
                map.notes.push(format!(
                    "{screen}: {failure}. {detail}. Continuing from verified screens."
                ));
                save_map(self.dir, map).await?;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => {
                let detail = format!("{screen}: {failure}. Device recovery failed: {error}");
                map.notes.push(detail.clone());
                save_map(self.dir, map).await?;
                Err(detail)
            }
        }
    }

    async fn native_tree(&self) -> Result<(Vec<UiElement>, bool), String> {
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

    async fn flutter_tree(&self) -> Result<Vec<UiElement>, String> {
        self.timing
            .measure("ui_tree", self.robot.list_flutter_elements(&self.device))
            .await
            .map_err(|error| error.to_string())
    }

    async fn observe(&self) -> Result<Observation, String> {
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

    async fn scroll_slice(&self, viewport: &Viewport, dy: i32) -> Result<Observation, String> {
        self.check()?;
        if self.scope.is_mobile() {
            self.context().await?;
        }
        self.timing
            .measure("viewport_scroll", async {
                if let Some(gesture) = viewport.android_swipe(dy) {
                    self.robot
                        .swipe(
                            &self.device,
                            viewport.screen_point(gesture.from),
                            viewport.screen_point(gesture.to),
                            Some(gesture.duration_ms),
                        )
                        .await
                } else {
                    self.robot
                        .scroll(&self.device, viewport.center(), 0., f64::from(dy))
                        .await
                }
            })
            .await
            .map_err(|error| format!("Could not scroll the selected viewport: {error}"))?;
        tokio::time::sleep(Duration::from_millis(SCROLL_SETTLE_MS)).await;
        let mut previous = None::<Observation>;
        for _ in 0..4 {
            let next = self.observe().await?;
            if next
                .native_viewport
                .as_ref()
                .is_some_and(|current| viewport.same_container(current))
            {
                if previous
                    .as_ref()
                    .is_some_and(|last| same_slice(last, &next))
                {
                    return Ok(next);
                }
                previous = Some(next);
            } else {
                previous = None;
            }
            // Wait for the current wheel frame to finish without sending more input.
            tokio::time::sleep(Duration::from_millis(140)).await;
        }
        Err("The selected scroll viewport did not stabilize after scrolling.".into())
    }

    async fn restore_slice(
        &self,
        anchor: &Observation,
        mut current: Observation,
        reverse: i32,
    ) -> Result<(), String> {
        let viewport = anchor.native_viewport.as_ref().expect("viewport anchor");
        let mobile = self.scope.is_mobile() || matches!(self.scope, Scope::IosNative);
        let mut visited = HashSet::new();
        for step in 0usize.. {
            if same_slice(anchor, &current) {
                return Ok(());
            }
            if !mobile && step >= usize::from(MAX_STEPS_PER_DIRECTION) {
                break;
            }
            let previous = current
                .native_viewport
                .as_ref()
                .filter(|v| viewport.same_container(v))
                .ok_or("The scroll viewport is no longer available.")?
                .clone();
            if !visited.insert((current.fingerprint, previous.layout_signature())) {
                break;
            }
            current = self.scroll_slice(viewport, reverse).await?;
            if current
                .native_viewport
                .as_ref()
                .is_some_and(|v| v.same_layout(&previous))
            {
                break;
            }
        }
        if same_slice(anchor, &current) {
            Ok(())
        } else {
            Err("Could not restore the original viewport safely.".into())
        }
    }

    async fn probe_viewport(&self, anchor: &mut Observation) -> Result<ViewportProbe, String> {
        let viewport = anchor.native_viewport.as_ref().expect("viewport anchor");
        let mut inventory = anchor.controls.clone();
        let mut hit_limit = false;
        let mut stopped = None;
        let mobile = self.scope.is_mobile() || matches!(self.scope, Scope::IosNative);
        'directions: for delta in [viewport.step(), -viewport.step()] {
            let mut previous = viewport.clone();
            let mut slices: Vec<(u64, Viewport)> = Vec::new();
            let mut visited = HashSet::from([(anchor.fingerprint, viewport.layout_signature())]);
            let mut last = None;
            for step in 0usize.. {
                if !mobile && step >= usize::from(MAX_STEPS_PER_DIRECTION) {
                    hit_limit = true;
                    break;
                }
                let current = match self.scroll_slice(viewport, delta).await {
                    Ok(current) => current,
                    Err(error) => {
                        let latest = self.observe().await.map_err(|_| error.clone())?;
                        if !same_slice(anchor, &latest) {
                            if !latest.native_viewport.as_ref().is_some_and(|viewport| {
                                slices.iter().any(|(fingerprint, known)| {
                                    *fingerprint == latest.fingerprint
                                        && known.same_position(viewport)
                                })
                            }) {
                                return Err(error);
                            }
                            self.restore_slice(anchor, latest, -delta).await?;
                        }
                        stopped = Some(error);
                        break 'directions;
                    }
                };
                let slice = current
                    .native_viewport
                    .as_ref()
                    .expect("validated viewport");
                if slice.same_layout(&previous)
                    && last
                        .as_ref()
                        .map_or(anchor.fingerprint, |previous: &Observation| {
                            previous.fingerprint
                        })
                        == current.fingerprint
                {
                    if last.is_some() {
                        last = Some(current);
                    }
                    break;
                }
                if !visited.insert((current.fingerprint, slice.layout_signature())) {
                    last = Some(current);
                    stopped =
                        Some("Viewport discovery returned to an already observed position.".into());
                    break;
                }
                previous = slice.clone();
                slices.push((current.fingerprint, slice.clone()));
                let plan = Reveal {
                    anchor: viewport.clone(),
                    delta,
                    slices: slices.clone(),
                };
                for row in current
                    .controls
                    .iter()
                    .filter(|row| !row.lookup.is_empty() && viewport.contains(row.center()))
                {
                    let known = inventory
                        .iter()
                        .position(|existing| existing.key == row.key);
                    if let Some(index) = known {
                        let existing = &mut inventory[index];
                        if existing.control.label != row.control.label
                            || row.control.note.as_deref()
                                == Some("Selector is ambiguous in this screen.")
                        {
                            existing.action = None;
                            existing.control.status = "skipped".into();
                            existing.control.note =
                                Some("Selector is ambiguous across viewport slices.".into());
                        }
                        continue;
                    }
                    if inventory.len() >= 160 {
                        hit_limit = true;
                        break;
                    }
                    let mut row = row.clone();
                    row.control.id = format!("c{}", inventory.len() + 1);
                    row.control.initially_offscreen = Some(true);
                    row.reveal = Some(plan.clone());
                    inventory.push(row);
                }
                last = Some(current);
                if hit_limit && mobile {
                    break;
                }
            }
            if let Some(last) = last
                && let Err(error) = self.restore_slice(anchor, last, -delta).await
            {
                if mobile
                    && self.options.reset_app_id.is_some()
                    && error == "Could not restore the original viewport safely."
                {
                    // Retain what was actually observed. Each recorded action
                    // must still verify its original branch/anchor separately.
                    stopped = Some(error);
                    break;
                }
                return Err(error);
            }
            if hit_limit && mobile {
                break;
            }
        }
        let revealed = inventory.len() - anchor.controls.len();
        anchor.controls = inventory;
        Ok(ViewportProbe {
            revealed,
            hit_limit,
            stopped,
        })
    }

    async fn reset_after_reveal(
        &self,
        row: &LiveControl,
        anchor: &Observation,
        current: Observation,
    ) -> Result<Observation, String> {
        if let Some(plan) = &row.reveal
            && let Some(viewport) = &current.native_viewport
            && plan.slices.iter().any(|(fingerprint, known)| {
                *fingerprint == current.fingerprint && known.same_replay_anchor(viewport)
            })
        {
            self.restore_slice(anchor, current, -plan.delta).await?;
            return self.observe().await;
        }
        Ok(current)
    }

    async fn settled(&self) -> Result<Observation, String> {
        tokio::time::sleep(Duration::from_millis(self.options.delay())).await;
        self.settled_after_wait().await
    }

    async fn settled_after_wait(&self) -> Result<Observation, String> {
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

    async fn settled_entry(&self) -> Result<Observation, String> {
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

    async fn observe_known(&self, accepted: &[u64]) -> Result<Observation, String> {
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

    async fn wait_for_mobile_relaunch(&self, accepted: &[u64]) -> Result<Observation, String> {
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

    async fn wait_for_window_entry(&self, root: u64, expected: u64) -> Result<Observation, String> {
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

    async fn perform(&mut self, row: &LiveControl, action: &str) -> Result<(), String> {
        self.check()?;
        self.input_budget -= 1;
        let device = &self.device;
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
        control: Option<&LiveControl>,
    ) -> Result<Observation, String> {
        // A tap can change the layout without changing the screen signature.
        // For an opted-in window fixture, reuse the normal relaunch path if a
        // recorded scroll action cannot start at the current viewport slice.
        let anchor = control.filter(|control| {
            control.reveal.is_some()
                && (matches!(self.scope, Scope::WindowFixture)
                    || self.options.reset_app_id.is_some())
        });
        let ready = |here: &Observation| {
            here.fingerprint == expected
                && anchor.is_none_or(|control| at_scroll_anchor(control, here))
        };
        // The previous action already verified this unchanged screen. The
        // action runner still checks the foreground and resolves its selector
        // against a fresh tree before sending input.
        if let Some(previous) = self.last_same_screen.take()
            && ready(&previous)
        {
            return Ok(previous);
        }
        let mut here = self.observe().await?;
        if ready(&here) {
            return Ok(here);
        }

        if here.fingerprint != expected
            && !matches!(self.scope, Scope::Native)
            && let Some(route) = known_route(navigation, here.fingerprint, expected)
                .filter(|route| route.len() <= path.len() + 1)
        {
            here = match self.replay_path(here, &route).await {
                Ok(restored) => restored,
                Err(_) => self.observe().await?,
            };
            if ready(&here) {
                return Ok(here);
            }
        }

        if let Scope::Web { start } = &self.scope {
            self.check()?;
            self.input_budget -= 1;
            let device = &self.device;
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
        } else if matches!(self.scope, Scope::WindowFixture) {
            self.check()?;
            self.input_budget -= 1;
            self.window_valid = false;
            self.device = self
                .timing
                .measure(
                    "app_relaunch",
                    self.robot.restart_window_fixture(&self.device),
                )
                .await
                .map_err(|error| {
                    format!("Could not relaunch the selected macOS fixture: {error}")
                })?;
            self.window_valid = true;
            here = self.wait_for_window_entry(root, expected).await?;
        } else if let Some(app) = &self.options.reset_app_id {
            self.check()?;
            self.input_budget -= 1;
            let device = &self.device;
            here =
                self.timing
                    .measure("app_relaunch", async {
                        self.robot
                            .terminate_app(device, app)
                            .await
                            .map_err(|error| format!("Could not stop the fixture app: {error}"))?;
                        self.robot.launch_app(device, app).await.map_err(|error| {
                            format!("Could not relaunch the fixture app: {error}")
                        })?;
                        self.wait_for_mobile_relaunch(&[root, expected]).await
                    })
                    .await?;
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
            let row = if let Some(row) = &step.revealed_control {
                at_scroll_anchor(row, &here).then_some(row)
            } else {
                here.controls
                    .iter()
                    .find(|c| c.key == step.key && c.action == Some(step.action))
            }
            .ok_or("A control on the recorded route is no longer uniquely available.")?;
            if step.initially_offscreen == Some(true) {
                self.check()?;
                self.input_budget -= 1;
                replay::restore(
                    self.robot,
                    &self.scope,
                    &self.device,
                    self.dir,
                    row.replay_action(
                        self.options,
                        step.initially_offscreen,
                        self.flutter.get() == Some(&true),
                    ),
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

fn is_mobile_transport_error(error: &str) -> bool {
    [
        "device offline",
        "adb protocol fault",
        "adb exited with status 255",
        "error: closed",
        "transport error",
        "connection reset",
        "adb devices timed out",
        "device not found:",
        "iOS XCTest command ",
        "iOS XCTest agent disconnected",
        "iOS XCTest agent last contacted the bridge",
        "no authenticated iOS XCTest agent",
        "XCTest could not reach the Mac bridge",
    ]
    .iter()
    .any(|message| error.contains(message))
}

struct ViewportProbe {
    revealed: usize,
    hit_limit: bool,
    stopped: Option<String>,
}

fn same_slice(anchor: &Observation, current: &Observation) -> bool {
    anchor.fingerprint == current.fingerprint
        && anchor
            .native_viewport
            .as_ref()
            .zip(current.native_viewport.as_ref())
            .is_some_and(|(old, new)| old.same_position(new))
}

fn at_scroll_anchor(row: &LiveControl, here: &Observation) -> bool {
    row.reveal.as_ref().is_some_and(|plan| {
        here.native_viewport
            .as_ref()
            .is_some_and(|viewport| plan.anchor.same_replay_anchor(viewport))
    })
}

fn candidate(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(|s| s.lines().map(str::trim).find(|s| !s.is_empty()))
        .map(|s| clipped(s, 160))
}

pub(super) fn is_xctest_node_id(id: &str) -> bool {
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

async fn foreground_app(
    robot: &dyn Robot,
    device: &str,
    platform: &Platform,
) -> Result<Option<String>, String> {
    if *platform == Platform::Ios {
        let status = robot
            .ui_runner_status(device)
            .await
            .map_err(|error| error.to_string())?;
        if !status.ready {
            return Err(format!(
                "Could not verify the iOS foreground app: {}",
                status.detail
            ));
        }
        return Ok(status.target_app_id);
    }
    let context = robot
        .stream_context(device)
        .await
        .map_err(|error| error.to_string())?;
    Ok(context["appId"]
        .as_str()
        .filter(|app| !app.is_empty())
        .map(str::to_owned))
}

async fn classify_scope(robot: &dyn Robot, opt: &Options) -> Result<Scope, String> {
    let device = &opt.device_id;
    if device.starts_with("chrome:") {
        if opt.reset_app_id.is_some() {
            return Err("Chrome restores the entry URL and does not use resetAppId.".into());
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
        if device.split(':').count() != 3 {
            return Err("Select a desktop process and window before collecting a map.".into());
        }
        if let Some(app) = &opt.reset_app_id {
            if !device.starts_with("macos:") {
                return Err("resetAppId is only supported for macOS and mobile fixtures.".into());
            }
            robot
                .bind_window_fixture(device, app)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(Scope::WindowFixture);
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
            let app = foreground_app(robot, device, platform)
                .await?
                .ok_or("Open the mobile app before mapping.")?;
            if opt
                .reset_app_id
                .as_deref()
                .is_some_and(|reset| reset != app)
            {
                return Err("resetAppId must match the currently open app.".into());
            }
            Ok(Scope::Mobile {
                app,
                platform: platform.clone(),
            })
        }
        Platform::Ios => Ok(Scope::IosNative),
        _ => Err("Select a specific Chrome tab or desktop window.".into()),
    }
}

async fn record(
    engine: &mut Engine<'_>,
    map: &mut UiMap,
    obs: &mut Observation,
    same_controls_as: Option<&str>,
    checkpoint: (u64, &[Arc<PathStep>], &Navigation),
) -> Result<String, String> {
    let id = format!("s{}", map.screens.len() + 1);
    let viewport = if matches!(engine.scope, Scope::Web { .. }) {
        engine
            .robot
            .web_viewport(&engine.device)
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
    let screenshot = match engine
        .timing
        .measure(
            "screen_screenshot",
            tokio::time::timeout(
                Duration::from_secs(10),
                engine.robot.take_screenshot(&engine.device),
            ),
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
                && tokio::fs::write(engine.dir.join(format!("{id}.png")), png)
                    .await
                    .is_ok()
        }
        _ => false,
    };
    if obs.ambiguous_viewport {
        map.notes.push(format!("{id}: Could not identify a unique native scroll viewport. Only visible controls were inventoried."));
    }
    let native_viewport = if same_controls_as.is_none() && obs.native_viewport.is_some() {
        let mut discovery = engine
            .timing
            .measure("viewport_discovery", engine.probe_viewport(obs))
            .await;
        if let Err(error) = &discovery
            && engine.recover_at(map, &id, error).await?
        {
            let (root, path, navigation) = checkpoint;
            let expected = obs.fingerprint;
            *obs = engine
                .restore(expected, root, path, navigation, None)
                .await?;
            if obs.native_viewport.is_some() {
                discovery = engine
                    .timing
                    .measure("viewport_discovery", engine.probe_viewport(obs))
                    .await;
            } else {
                discovery =
                    Err("The recovered screen did not expose its original viewport.".into());
            }
        }
        match discovery {
            Ok(ViewportProbe {
                revealed: revealed_controls,
                hit_limit,
                stopped,
            }) => {
                if let Some(reason) = stopped {
                    map.notes.push(format!(
                        "{id}: Native viewport discovery stopped early. {reason}"
                    ));
                }
                if hit_limit {
                    map.notes.push(format!("{id}: Native viewport discovery reached its bounded scroll or inventory limit."));
                }
                Some(super::model::NativeViewport {
                    bounds: obs.native_viewport.as_ref().unwrap().bounds.clone(),
                    revealed_controls,
                    hit_limit,
                })
            }
            Err(error) => {
                map.notes.push(format!("{id}: {error}"));
                for row in obs.controls.iter_mut().filter(|row| row.action.is_some()) {
                    row.action = None;
                    row.control.status = "skipped".into();
                    row.control.note =
                        Some("Viewport discovery could not verify the entry position.".into());
                }
                None
            }
        }
    } else {
        None
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
        native_viewport,
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
    Ok(id)
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
    timing: &TimingLog,
) -> Result<(), String> {
    let scope = timing
        .measure("scope_setup", classify_scope(&**robot, opt))
        .await?;
    let recovery_boot_target = if scope.is_android() && opt.reset_app_id.is_some() {
        robot
            .recovery_boot_target(&opt.device_id)
            .await
            .map_err(|error| error.to_string())?
    } else {
        None
    };
    let mut engine = Engine {
        robot,
        dir,
        options: opt,
        device: opt.device_id.clone(),
        window_valid: true,
        scope,
        stop,
        timing,
        flutter: OnceLock::new(),
        deadline: opt
            .max_time_seconds
            .map(|seconds| Instant::now() + Duration::from_secs(seconds)),
        input_budget: opt.actions() * (opt.depth() + 2) + 20,
        last_same_screen: None,
        recovery_boot_target,
    };
    let mut root = timing
        .measure("entry_observation", engine.settled_entry())
        .await?;
    if (matches!(engine.scope, Scope::Web { .. }) || engine.scope.is_mobile())
        && root.controls.is_empty()
        && root.native_viewport.is_none()
    {
        map.notes
            .push("No controls were found on the entry page after waiting for it to load.".into());
    }
    if opt.device_id.starts_with("macos:")
        && engine.flutter.get() == Some(&false)
        && root.controls.iter().all(|row| row.lookup.is_empty())
    {
        map.notes.push(
            "No named controls were exposed by the selected macOS window. For a sandboxed Flutter app with DeviceBotFlutter enabled, configure its exact window endpoint before scanning.".into(),
        );
    }
    let root_fp = root.fingerprint;
    let mut navigation = Navigation::new();
    let root_id = record(
        &mut engine,
        map,
        &mut root,
        None,
        (root_fp, &[], &navigation),
    )
    .await?;
    let mut seen = HashMap::from([(root.fingerprint, root_id.clone())]);
    let mut control_signatures = HashMap::from([(root.control_signature, root_id.clone())]);
    let mut queue = VecDeque::from([(
        root.fingerprint,
        root_id,
        Vec::<Arc<PathStep>>::new(),
        root.controls,
    )]);
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
            let mut restored = timing
                .measure(
                    "restore_branch",
                    engine.restore(fingerprint, root_fp, &path, &navigation, Some(original)),
                )
                .await;
            if let Err(error) = &restored
                && engine.recover_at(map, &from, error).await?
            {
                restored = timing
                    .measure(
                        "restore_branch",
                        engine.restore(fingerprint, root_fp, &path, &navigation, Some(original)),
                    )
                    .await;
            }
            if map.device_id != engine.device {
                map.device_id.clone_from(&engine.device);
                save_map(dir, map).await?;
            }
            let here = match restored {
                Ok(here) => here,
                Err(error)
                    if engine.scope.is_mobile()
                        || matches!(engine.scope, Scope::WindowFixture) && engine.window_valid =>
                {
                    // Preserve the app and scan budget guards when an individual route drifts.
                    engine.context().await?;
                    if matches!(engine.scope, Scope::WindowFixture) {
                        engine.observe().await.map_err(|_| error.clone())?;
                    }
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
            let row = if original.reveal.is_some() {
                at_scroll_anchor(original, &here).then_some(original)
            } else {
                here.controls
                    .iter()
                    .find(|c| c.key == original.key && c.action == Some(action))
            };
            let Some(row) = row else {
                let note = if original.reveal.is_some() {
                    "Could not verify the recorded scroll position before input."
                } else {
                    "Control changed before input."
                };
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "skipped",
                    Some(note.into()),
                );
                continue;
            };
            map.attempted += 1;
            engine.input_budget -= 1;
            let name = format!("{from}-{}", original.control.id);
            let (evidence, error) = timing
                .measure(
                    "execute_action",
                    replay::run(
                        robot,
                        &engine.scope,
                        &engine.device,
                        dir,
                        &name,
                        row.replay_action(
                            opt,
                            original.control.initially_offscreen,
                            engine.flutter.get() == Some(&true),
                        ),
                        opt.delay(),
                    ),
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
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "failed",
                    Some(err.clone()),
                );
                save_map(dir, map).await?;
                if engine.recover_at(map, &from, &err).await? {
                    // The action may have reached the device before the
                    // disconnect. Never resend it as the interrupted attempt.
                    continue;
                }
                engine.context().await?;
                if row.reveal.is_some() {
                    let reset = async {
                        let current = engine.observe().await?;
                        engine.reset_after_reveal(row, &here, current).await
                    }
                    .await;
                    if let Err(error) = reset
                        && !engine.recover_at(map, &from, &error).await?
                    {
                        return Err(error);
                    }
                }
                continue;
            }
            let mut next = match timing
                // Successful action YAML has already waited before capturing evidence.
                .measure("settle_after_action", engine.settled_after_wait())
                .await
            {
                Ok(next) => next,
                Err(error) => {
                    mark(
                        map,
                        &from,
                        &original.control.id,
                        "failed",
                        Some(error.clone()),
                    );
                    save_map(dir, map).await?;
                    if engine.recover_at(map, &from, &error).await? {
                        continue;
                    }
                    return Err(error);
                }
            };
            if row.reveal.is_some() {
                match engine.reset_after_reveal(row, &here, next).await {
                    Ok(restored) => next = restored,
                    Err(error) => {
                        mark(
                            map,
                            &from,
                            &original.control.id,
                            "failed",
                            Some(error.clone()),
                        );
                        save_map(dir, map).await?;
                        if engine.recover_at(map, &from, &error).await? {
                            continue;
                        }
                        return Err(error);
                    }
                }
            }
            if next.fingerprint == fingerprint {
                mark(
                    map,
                    &from,
                    &original.control.id,
                    "no_change",
                    Some("No new screen signature observed.".into()),
                );
                if engine.scope.is_scoped_ios() {
                    engine.last_same_screen = Some(next);
                }
            } else {
                let step = Arc::new(PathStep {
                    key: original.key.clone(),
                    action,
                    before: fingerprint,
                    after: next.fingerprint,
                    initially_offscreen: original.control.initially_offscreen,
                    revealed_control: row.reveal.as_ref().map(|_| row.clone()),
                });
                let to = if let Some(to) = seen.get(&next.fingerprint) {
                    to.clone()
                } else if map.screens.len() < opt.screens() {
                    let representative =
                        if opt.expand_selection_states || next.native_viewport.is_some() {
                            None
                        } else {
                            control_signatures
                                .get(&next.control_signature)
                                .map(String::as_str)
                        };
                    let mut child_path = path.clone();
                    child_path.push(Arc::clone(&step));
                    let to = record(
                        &mut engine,
                        map,
                        &mut next,
                        representative,
                        (root_fp, &child_path, &navigation),
                    )
                    .await?;
                    seen.insert(next.fingerprint, to.clone());
                    if representative.is_none() {
                        control_signatures.insert(next.control_signature, to.clone());
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

//! Bounded exploration over Robot input and accessibility, without app-specific scripts.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use robot_core::{Platform, Point, Robot, UiElement};
use serde::Deserialize;
use tokio::time::Instant;
use url::Url;

use super::{
    Control, Transition, UiMap, replay, save_map,
    timing::TimingLog,
    viewport::{Reveal, Viewport},
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
    pub dismiss_keyboard_after_input: bool,
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
        if self.dismiss_keyboard_after_input && self.reset_app_id.is_none() {
            return Err("dismissKeyboardAfterInput requires resetAppId.".into());
        }
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
            self.input_value(options)
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

    fn input_value<'a>(&self, options: &'a Options) -> Option<&'a str> {
        self.lookup
            .iter()
            .find_map(|key| options.inputs.get(key))
            .map(String::as_str)
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

    pub(super) fn is_native_mobile(&self) -> bool {
        self.is_mobile() || matches!(self, Self::IosNative)
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

mod evidence;
mod navigation;
mod observation;
mod reveal;
mod scope;

use evidence::record;
pub(super) use observation::is_xctest_node_id;
use reveal::at_scroll_anchor;
use scope::{classify_scope, foreground_app, parse_web, web_location};

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
                .input_value(self.options)
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
            self.dismiss_keyboard_after_input(row).await?;
        }
        Ok(())
    }

    async fn dismiss_keyboard_after_input(&self, row: &LiveControl) -> Result<(), String> {
        if !self.options.dismiss_keyboard_after_input || row.action != Some("input") {
            return Ok(());
        }
        let app_id = self
            .options
            .reset_app_id
            .as_deref()
            .ok_or("dismissKeyboardAfterInput requires resetAppId.")?;
        let value = row
            .input_value(self.options)
            .ok_or("No synthetic input is configured for this editor.")?;
        self.context().await?;
        robot_flutter_bridge::dismiss_keyboard(&self.device, app_id, value)
            .await
            .map_err(|error| error.to_string())?;
        self.context().await?;
        Ok(())
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
    if opt.dismiss_keyboard_after_input && !scope.is_mobile() {
        return Err("dismissKeyboardAfterInput requires a scoped native mobile app.".into());
    }
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
            let revealed;
            let row = if original.reveal.is_some() && engine.scope.is_native_mobile() {
                revealed = match engine.reveal_mobile_control(original, &here).await {
                    Ok(row) => row,
                    Err(error) => {
                        map.attempted += 1;
                        mark(
                            map,
                            &from,
                            &original.control.id,
                            "failed",
                            Some(error.clone()),
                        );
                        save_map(dir, map).await?;
                        if !engine.recover_at(map, &from, &error).await? {
                            engine.check()?;
                        }
                        continue;
                    }
                };
                Some(&revealed)
            } else if original.reveal.is_some() {
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
                .measure("settle_after_action", async {
                    engine.dismiss_keyboard_after_input(row).await?;
                    engine.settled_after_wait().await
                })
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
                    revealed_control: original.reveal.as_ref().map(|_| original.clone()),
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

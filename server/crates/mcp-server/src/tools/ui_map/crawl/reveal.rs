//! Discovers and restores controls outside the visible viewport.

use super::super::viewport::{MAX_STEPS_PER_DIRECTION, Reveal, SCROLL_SETTLE_MS, Viewport};
use super::{Engine, LiveControl, Observation};
use std::collections::HashSet;
use std::time::Duration;

impl Engine<'_> {
    pub(super) async fn scroll_slice(
        &self,
        viewport: &Viewport,
        dy: i32,
    ) -> Result<Observation, String> {
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

    pub(super) async fn restore_slice(
        &self,
        anchor: &Observation,
        mut current: Observation,
        reverse: i32,
    ) -> Result<(), String> {
        let viewport = anchor.native_viewport.as_ref().expect("viewport anchor");
        let mobile = self.scope.is_native_mobile();
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

    pub(super) async fn probe_viewport(
        &self,
        anchor: &mut Observation,
    ) -> Result<ViewportProbe, String> {
        let viewport = anchor.native_viewport.as_ref().expect("viewport anchor");
        let mut inventory = anchor.controls.clone();
        let mut hit_limit = false;
        let mut stopped = None;
        let mobile = self.scope.is_native_mobile();
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

    pub(super) async fn reveal_mobile_control(
        &mut self,
        original: &LiveControl,
        here: &Observation,
    ) -> Result<LiveControl, String> {
        let plan = original.reveal.as_ref().expect("recorded reveal");
        let viewport = here
            .native_viewport
            .as_ref()
            .filter(|viewport| plan.anchor.same_container(viewport))
            .ok_or("The recorded viewport is no longer available.")?;
        let mut actual = Reveal {
            anchor: viewport.clone(),
            delta: plan.delta,
            slices: Vec::new(),
        };
        if let Some(row) = visible_reveal_target(original, here) {
            let mut row = row.clone();
            row.reveal = Some(actual);
            return Ok(row);
        }
        if !at_scroll_anchor(original, here) {
            return Err("Could not verify the recorded scroll position before input.".into());
        }

        let mut previous = viewport.clone();
        let mut visited = HashSet::from([viewport.layout_signature()]);
        loop {
            self.check()?;
            let next = self.scroll_slice(&previous, plan.delta).await?;
            self.input_budget -= 1;
            let slice = next.native_viewport.as_ref().expect("validated viewport");
            actual.slices.push((next.fingerprint, slice.clone()));
            if let Some(row) = visible_reveal_target(original, &next) {
                let mut row = row.clone();
                row.reveal = Some(actual);
                return Ok(row);
            }
            if !visited.insert(slice.layout_signature()) {
                return Err(
                    "The viewport stopped progressing before the control became visible.".into(),
                );
            }
            previous = slice.clone();
        }
    }

    pub(super) async fn reset_after_reveal(
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
}

pub(super) struct ViewportProbe {
    pub(super) revealed: usize,
    pub(super) hit_limit: bool,
    pub(super) stopped: Option<String>,
}

pub(super) fn same_slice(anchor: &Observation, current: &Observation) -> bool {
    anchor.fingerprint == current.fingerprint
        && anchor
            .native_viewport
            .as_ref()
            .zip(current.native_viewport.as_ref())
            .is_some_and(|(old, new)| old.same_position(new))
}

pub(super) fn at_scroll_anchor(row: &LiveControl, here: &Observation) -> bool {
    row.reveal.as_ref().is_some_and(|plan| {
        here.native_viewport
            .as_ref()
            .is_some_and(|viewport| plan.anchor.same_replay_anchor(viewport))
    })
}

pub(super) fn visible_reveal_target<'a>(
    original: &LiveControl,
    here: &'a Observation,
) -> Option<&'a LiveControl> {
    let plan = original.reveal.as_ref()?;
    let viewport = here
        .native_viewport
        .as_ref()
        .filter(|viewport| plan.anchor.same_container(viewport))?;
    here.controls.iter().find(|row| {
        row.key == original.key && row.action == original.action && viewport.contains(row.center())
    })
}

//! Recovers a device and restores an observed navigation path.

use super::super::{UiMap, claim_recovered_device, replay, save_map};
use super::reveal::{at_scroll_anchor, visible_reveal_target};
use super::{
    Engine, LiveControl, Navigation, Observation, PathStep, Scope, is_mobile_transport_error,
    known_route,
};
use robot_core::{DeviceState, Platform};
use std::collections::HashSet;
use std::sync::Arc;

impl Engine<'_> {
    pub(super) async fn recover_target(
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
                            && recovered.replace(device.id).is_some()
                        {
                            return Err(format!("More than one device matches {boot_target}."));
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

    pub(super) async fn recover_at(
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

    pub(super) async fn restore(
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
        let native_mobile = self.scope.is_native_mobile();
        let ready = |here: &Observation| {
            here.fingerprint == expected
                && anchor.is_none_or(|control| {
                    at_scroll_anchor(control, here)
                        || native_mobile && visible_reveal_target(control, here).is_some()
                })
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

    pub(super) async fn replay_path(
        &mut self,
        mut here: Observation,
        path: &[Arc<PathStep>],
    ) -> Result<Observation, String> {
        for step in path {
            if here.fingerprint != step.before {
                return Err("The recorded route now leads to a different screen.".into());
            }
            let revealed;
            let row = if let Some(row) = &step.revealed_control {
                if self.scope.is_native_mobile() {
                    revealed = self.reveal_mobile_control(row, &here).await?;
                    Some(&revealed)
                } else {
                    at_scroll_anchor(row, &here).then_some(row)
                }
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
                self.dismiss_keyboard_after_input(row).await?;
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

//! Records discovered screens and captures their preview evidence.

use super::super::{Screen, UiMap};
use super::reveal::ViewportProbe;
use super::{Engine, Navigation, Observation, PathStep, Scope};
use std::sync::Arc;
use std::time::Duration;

pub(super) async fn record(
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
            if png.len() <= super::super::MAX_IMAGE_BYTES
                && png.starts_with(b"\x89PNG\r\n\x1a\n") =>
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
                Some(super::super::model::NativeViewport {
                    bounds: obs.native_viewport.as_ref().unwrap().bounds,
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

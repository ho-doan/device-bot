//! Simulator Live uses native ScreenCaptureKit frames from the selected simulator's
//! visible screen. New Xcodes no longer stream `simctl recordVideo` to stdout.
use super::*;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct LiveTarget {
    pid: i32,
    window_id: u32,
    crop: [f64; 4],
    width: u32,
    height: u32,
    app_id: String,
    host_width: f64,
    host_height: f64,
}

impl LiveTarget {
    fn encoded_size(&self) -> (u32, u32) {
        (self.width & !1, self.height & !1)
    }

    fn validate(&self) -> Result<(), RobotError> {
        if self.pid <= 0
            || self.window_id == 0
            || self.crop.iter().any(|v| !v.is_finite())
            || !self.host_width.is_finite()
            || !self.host_height.is_finite()
            || self.host_width < 2.
            || self.host_height < 2.
            || self.crop[0] < 0.
            || self.crop[1] < 0.
            || self.crop[2] < 2.
            || self.crop[3] < 2.
            || self.crop[0] + self.crop[2] > self.host_width
            || self.crop[1] + self.crop[3] > self.host_height
            || !(2..=16384).contains(&self.width)
            || !(2..=16384).contains(&self.height)
        {
            return Err(fail("The simulator screen crop is invalid."));
        }
        Ok(())
    }
}

fn fail(message: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("iOS Simulator Live: {message}"))
}

fn selected_name(devices: &Value, id: &str) -> Result<String, RobotError> {
    let rows: Vec<_> = devices["devices"]
        .as_object()
        .into_iter()
        .flat_map(|runtimes| runtimes.values())
        .filter_map(Value::as_array)
        .flatten()
        .collect();
    let selected = rows
        .iter()
        .find(|row| row["udid"].as_str() == Some(id))
        .ok_or_else(|| RobotError::DeviceNotFound(id.into()))?;
    if selected["state"].as_str() != Some("Booted") {
        return Err(fail("Boot the selected simulator before opening Live."));
    }
    let name = selected["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("The selected simulator has no display name."))?;
    // A host window is selected by name only after proving that it identifies
    // exactly one simulator. Never bind another UDID with a duplicate name.
    if rows
        .iter()
        .filter(|row| row["name"].as_str() == Some(name))
        .count()
        != 1
    {
        return Err(fail(
            "Simulator names are ambiguous. Give the selected simulator a unique name before opening Live.",
        ));
    }
    Ok(name.into())
}

fn logical_size(value: &Value) -> Result<(u32, u32), RobotError> {
    let dimension = |key: &str| {
        value[key]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 2. && *v <= 16384. && v.fract() == 0.)
            .map(|v| v as u32)
            .ok_or_else(|| {
                fail(format!(
                    "The XCTest helper did not return a valid {key}; rebuild the helper."
                ))
            })
    };
    Ok((dimension("logicalWidth")?, dimension("logicalHeight")?))
}

impl IosSimulatorRobot {
    async fn resolve_live_target(&self, id: &str) -> Result<LiveTarget, RobotError> {
        Self::valid_device_id(id)?;
        if !cfg!(target_os = "macos") {
            return Err(RobotError::Unsupported(
                "Simulator Live requires macOS and Xcode.".into(),
            ));
        }
        let devices = self
            .simctl_json(&[
                "simctl".into(),
                "list".into(),
                "devices".into(),
                "--json".into(),
            ])
            .await?;
        let name = selected_name(&devices, id)?;
        let app_id = self.active_app(id).await?;
        let screen = self
            .run_xctest(id, &app_id, "screen-geometry", &[], None)
            .await?;
        let (width, height) = logical_size(&screen)?;
        let resolver = prepare_resolver().await?;
        let mut command = Command::new(resolver);
        command
            .arg(&name)
            .arg(width.to_string())
            .arg(height.to_string());
        if let Some(path) = std::env::var_os("DEVICE_BOT_SIMULATOR_LIVE_TARGETS") {
            let config: Value = serde_json::from_slice(&tokio::fs::read(path).await.map_err(fail)?)
                .map_err(fail)?;
            if let Some(target) = config.get(id) {
                command.arg(target.to_string());
            }
        }
        let output =
            tokio::time::timeout(Duration::from_secs(15), command.kill_on_drop(true).output())
                .await
                .map_err(|_| fail("Simulator display accessibility lookup timed out."))?
                .map_err(fail)?;
        if !output.status.success() {
            return Err(fail(String::from_utf8_lossy(&output.stderr).trim()));
        }
        let mut target: LiveTarget = serde_json::from_slice(&output.stdout).map_err(fail)?;
        target.width = width;
        target.height = height;
        target.app_id = app_id;
        target.validate()?;
        Ok(target)
    }

    pub(super) async fn live_context(&self, id: &str) -> Result<Value, RobotError> {
        let target = self.resolve_live_target(id).await?;
        robot_macos::prepare_window_capture().await?;
        self.live_targets
            .lock()
            .await
            .insert(id.into(), target.clone());
        let (width, height) = target.encoded_size();
        Ok(json!({"deviceId":id, "appId":target.app_id,
            "logicalWidth":target.width, "logicalHeight":target.height,
            "originX":0, "originY":0, "width":width, "height":height,
            "hostProcessId":target.pid, "hostWindowId":target.window_id,
            "captureBackend":"simulator-window-screencapturekit"}))
    }

    pub(super) async fn start_live_video(
        &self,
        id: &str,
    ) -> Result<robot_core::VideoStream, RobotError> {
        let expected = self
            .live_targets
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| fail("Request stream context before starting simulator capture."))?;
        let current = self.resolve_live_target(id).await?;
        if current != expected {
            return Err(fail(
                "The simulator app, window, or screen geometry changed. Reopen Live.",
            ));
        }
        let (width, height) = current.encoded_size();
        let mut source = robot_macos::start_window_capture(
            current.pid,
            current.window_id,
            width,
            height,
            Some(current.crop),
        )
        .await?;
        let resolver = prepare_resolver().await?;
        let devices = self
            .simctl_json(&[
                "simctl".into(),
                "list".into(),
                "devices".into(),
                "--json".into(),
            ])
            .await?;
        let name = selected_name(&devices, id)?;
        let (sender, frames) = tokio::sync::mpsc::channel(2);
        let (error_sender, mut errors) = tokio::sync::mpsc::channel(1);
        let closed = sender.clone();
        let monitor_target = current.clone();
        let (monitor_stop, mut monitor_stopped) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            loop {
                tokio::select! { _ = monitor_stopped.changed() => break, _ = closed.closed() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
                let check = Command::new(&resolver)
                    .arg("--check")
                    .arg(monitor_target.pid.to_string())
                    .arg(monitor_target.window_id.to_string())
                    .arg(&name)
                    .arg(monitor_target.host_width.to_string())
                    .arg(monitor_target.host_height.to_string())
                    .kill_on_drop(true)
                    .output();
                let result = tokio::select! { _ = monitor_stopped.changed() => break, _ = closed.closed() => break, result = tokio::time::timeout(Duration::from_secs(5),check) => result };
                if !matches!(result, Ok(Ok(ref output)) if output.status.success()) {
                    let _ = error_sender
                        .send(fail(
                            "The simulator source changed device, closed, or resized. Reopen Live.",
                        ))
                        .await;
                    break;
                }
            }
        });
        tokio::spawn(async move {
            let _monitor_stop = monitor_stop;
            loop {
                let next = tokio::select! {
                    _ = sender.closed() => break,
                    error = errors.recv() => { if let Some(error)=error { let _=sender.send(Err(error)).await; } break; },
                    next = source.frames.recv() => next,
                };
                let Some(frame) = next else { break };
                if sender.send(frame).await.is_err() {
                    break;
                }
            }
        });
        Ok(robot_core::VideoStream {
            width,
            height,
            frames,
        })
    }
}

async fn prepare_resolver() -> Result<PathBuf, RobotError> {
    use std::hash::{Hash, Hasher};
    let source = include_str!("live_target.swift");
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    let directory =
        std::env::temp_dir().join(format!("device-bot-simulator-live-{:x}", hash.finish()));
    let binary = directory.join("resolve");
    if binary.is_file() {
        return Ok(binary);
    }
    static COMPILE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = COMPILE.lock().await;
    if binary.is_file() {
        return Ok(binary);
    }
    tokio::fs::create_dir_all(&directory).await.map_err(fail)?;
    let input = directory.join("resolve.swift");
    tokio::fs::write(&input, source).await.map_err(fail)?;
    let temporary = directory.join("resolve.new");
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("xcrun")
            .args(["swiftc", "-swift-version", "5", "-O"])
            .arg(input)
            .arg("-module-cache-path")
            .arg(directory.join("modules"))
            .arg("-o")
            .arg(&temporary)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| fail("Simulator display resolver compilation timed out."))?
    .map_err(fail)?;
    if !output.status.success() {
        return Err(fail(format!(
            "Could not compile the display resolver: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    tokio::fs::rename(temporary, &binary).await.map_err(fail)?;
    Ok(binary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn prepared_live_context_is_not_inherited_by_another_session() {
        let robot = IosSimulatorRobot::new("xcrun", std::env::temp_dir());
        robot.live_targets.lock().await.insert(
            "device".into(),
            LiveTarget {
                pid: 1,
                window_id: 2,
                crop: [0., 0., 100., 200.],
                width: 100,
                height: 200,
                app_id: "test.app".into(),
                host_width: 100.,
                host_height: 200.,
            },
        );
        let session = robot.fork_session().unwrap();
        let error = match session.start_video_stream("device").await {
            Ok(_) => panic!("another session inherited Live context"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("Request stream context"));
    }
    #[test]
    fn odd_logical_dimensions_survive_encoder_alignment_and_crop_is_bounded() {
        let target = LiveTarget {
            pid: 1,
            window_id: 2,
            crop: [10., 20., 196.5, 426.],
            width: 393,
            height: 852,
            app_id: "test.app".into(),
            host_width: 240.,
            host_height: 480.,
        };
        assert!(target.validate().is_ok());
        assert_eq!(target.encoded_size(), (392, 852));
        assert_eq!(target.width, 393);
        let mut invalid = target.clone();
        invalid.crop[2] = invalid.host_width;
        assert!(invalid.validate().is_err());
        let mut invalid = target.clone();
        invalid.crop[3] = invalid.host_height;
        assert!(invalid.validate().is_err());
        let mut invalid = target;
        invalid.host_height = f64::NAN;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn selected_udid_must_be_booted_and_unambiguous() {
        let devices = json!({"devices":{"runtime":[{"udid":"one","name":"Test phone","state":"Booted"}, {"udid":"two","name":"Other phone","state":"Shutdown"}]}});
        assert_eq!(selected_name(&devices, "one").unwrap(), "Test phone");
        assert!(selected_name(&devices, "two").is_err());
        assert!(selected_name(&devices, "missing").is_err());
        let duplicate = json!({"devices":{"runtime":[{"udid":"one","name":"Phone","state":"Booted"}, {"udid":"two","name":"Phone","state":"Booted"}]}});
        assert!(selected_name(&duplicate, "one").is_err());
    }
    #[test]
    fn logical_dimensions_require_current_valid_helper_metadata() {
        assert_eq!(
            logical_size(&json!({"logicalWidth":402,"logicalHeight":874})).unwrap(),
            (402, 874)
        );
        for invalid in [
            json!({}),
            json!({"logicalWidth":0,"logicalHeight":874}),
            json!({"logicalWidth":402.5,"logicalHeight":874}),
        ] {
            assert!(logical_size(&invalid).is_err());
        }
    }
}

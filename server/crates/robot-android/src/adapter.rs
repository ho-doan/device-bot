//! Portable device operations backed by native platform modules.

use super::{
    AndroidRobot, CommandRunner, Ordering, TREE_DUMP_SEQUENCE, Value, android_coordinate,
    android_probe_readiness, encode_input_text, ensure_apk_path, ensure_app_id, ensure_device_id,
    ensure_web_url, json, parse_adb_installed_apps, parse_focused_ime_action,
    parse_uiautomator_xml, video,
};
use async_trait::async_trait;
use robot_core::{
    DeviceState, ElementType, ImeAction, Point, Robot, RobotDevice, RobotError, UiElement,
};

#[async_trait]
impl<R: CommandRunner> Robot for AndroidRobot<R> {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        let device = self
            .list_devices()
            .await?
            .into_iter()
            .find(|device| device.id == id)
            .ok_or_else(|| RobotError::DeviceNotFound(id.into()))?;
        let mut report = robot_core::CapabilityReport::new(id);
        report.capabilities.discovery =
            robot_core::Capability::available("Android device is discoverable");
        report.capabilities.observation = robot_core::Capability::unsupported(
            "Native application log and network observation is not implemented",
        );
        if device.state != DeviceState::Available {
            let blocked =
                robot_core::Capability::blocked("Connect, authorize and unlock the Android device");
            report.capabilities.launch = blocked.clone();
            report.capabilities.input = blocked.clone();
            report.capabilities.screenshot = blocked.clone();
            report.capabilities.elements = blocked.clone();
            report.capabilities.screen_recording = blocked;
        } else {
            // Query the live hierarchy through stdout: no file on the device is created.
            let command = |args: Vec<String>| async move {
                tokio::time::timeout(std::time::Duration::from_secs(8), self.run_adb(&args))
                    .await
                    .ok()
                    .and_then(Result::ok)
            };
            let prefix = || vec!["-s".to_owned(), id.to_owned()];
            let mut hierarchy_args = prefix();
            hierarchy_args
                .extend(["exec-out", "uiautomator", "dump", "/dev/stdout"].map(str::to_owned));
            let mut input_args = prefix();
            input_args.extend(["shell", "command", "-v", "input"].map(str::to_owned));
            let mut launch_args = prefix();
            launch_args.extend(["shell", "dumpsys", "activity", "activities"].map(str::to_owned));
            let mut screenshot_args = prefix();
            screenshot_args.extend(["exec-out", "screencap", "-p"].map(str::to_owned));
            let mut recording_args = prefix();
            recording_args.extend(["shell", "command", "-v", "screenrecord"].map(str::to_owned));
            let hierarchy_probe = async {
                let gate = {
                    let mut gates = self.tree_dump_locks.lock().await;
                    gates.entry(id.to_owned()).or_default().clone()
                };
                let Ok(_guard) =
                    tokio::time::timeout(std::time::Duration::from_secs(8), gate.lock()).await
                else {
                    return None;
                };
                command(hierarchy_args).await
            };
            let mut setsid_args = prefix();
            setsid_args.extend(["shell", "command", "-v", "setsid"].map(str::to_owned));
            let (hierarchy, input, launch, screenshot, recording, setsid) = tokio::join!(
                hierarchy_probe,
                command(input_args),
                command(launch_args),
                command(screenshot_args),
                command(recording_args),
                command(setsid_args)
            );
            let hierarchy_ok = hierarchy
                .as_ref()
                .is_some_and(|out| String::from_utf8_lossy(&out.stdout).contains("<hierarchy"));
            let input_ok = input.as_ref().is_some_and(|out| !out.stdout.is_empty());
            report.capabilities.elements = android_probe_readiness(
                hierarchy_ok,
                "Android UI hierarchy responded",
                "Unlock the device and check UI Automator",
            );
            report.capabilities.input = android_probe_readiness(
                hierarchy_ok && input_ok,
                "Android UI hierarchy and input command are responsive",
                "Unlock the device and check UI Automator and input command",
            );
            report.capabilities.launch = android_probe_readiness(
                launch.as_ref().is_some_and(|out| !out.stdout.is_empty()),
                "Android activity service is responsive",
                "Check Android activity service and device authorization",
            );
            report.capabilities.screenshot = android_probe_readiness(
                screenshot
                    .as_ref()
                    .is_some_and(|out| out.stdout.starts_with(b"\x89PNG\r\n\x1a\n")),
                "Android returned a PNG screenshot",
                "Check device display and screencap availability",
            );
            let recorder_tools = recording.as_ref().is_some_and(|out| !out.stdout.is_empty())
                && setsid.as_ref().is_some_and(|out| !out.stdout.is_empty());
            let screenshot_ok = screenshot
                .as_ref()
                .is_some_and(|out| out.stdout.starts_with(b"\x89PNG\r\n\x1a\n"));
            report.capabilities.screen_recording = if !recorder_tools {
                robot_core::Capability::blocked(
                    "Install or enable Android screenrecord and setsid on the device",
                )
            } else if !screenshot_ok || self.recordings.lock().await.contains_key(id) {
                robot_core::Capability::blocked(
                    "Check the device display and stop any active Device Bot recording",
                )
            } else {
                robot_core::Capability::available(
                    "Display and recording commands are present; recording or encoding can still fail",
                )
            };
        }
        Ok(report)
    }

    async fn start_video_stream(
        &self,
        device_id: &str,
    ) -> Result<robot_core::VideoStream, RobotError> {
        ensure_device_id(device_id)?;
        video::start(self.adb_path(), device_id, &self.video_controls).await
    }
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.discover_devices().await
    }

    async fn boot_device(&self, device_id: &str) -> Result<(), RobotError> {
        self.boot_emulator(device_id).await
    }
    async fn recovery_boot_target(&self, device_id: &str) -> Result<Option<String>, RobotError> {
        Ok(self
            .running_avd_name(device_id)
            .await
            .map(|name| format!("avd:{name}")))
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "pm".into(),
                "list".into(),
                "packages".into(),
                "-f".into(),
            ])
            .await?;
        let text = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!(
                "adb package listing returned invalid UTF-8: {error}"
            ))
        })?;
        Ok(json!({
            "platform": "android",
            "deviceId": device_id,
            "apps": parse_adb_installed_apps(&text),
        }))
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        ensure_device_id(device_id)?;
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create recording directory: {error}"))
            })?;

        let mut recordings = self.recordings.lock().await;
        if recordings.contains_key(device_id) {
            return Err(RobotError::InvalidInput(format!(
                "a screen recording is already running for device {device_id}"
            )));
        }

        let mut recording = self.next_recording(device_id);
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "rm".into(),
            "-f".into(),
            recording.device_path.clone(),
        ])
        .await?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                format!(
                    "setsid screenrecord --time-limit 1800 {} >/dev/null 2>&1 & echo $!",
                    recording.device_path
                ),
            ])
            .await?;
        let pid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                RobotError::CommandFailed(
                    "adb did not return a screenrecord process id; device may not support screenrecord"
                        .into(),
                )
            })?;
        recording.pid = pid;
        let artifact_path = recording.artifact_path.to_string_lossy().into_owned();
        recordings.insert(device_id.into(), recording);
        Ok(artifact_path)
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        ensure_device_id(device_id)?;
        let Some(recording) = self.recordings.lock().await.remove(device_id) else {
            return Err(RobotError::InvalidInput(format!(
                "no screen recording is running for device {device_id}"
            )));
        };
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            format!(
                "kill -INT {}; i=0; while kill -0 {} 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done",
                recording.pid, recording.pid
            ),
        ])
        .await?;
        let device_path = recording.device_path.clone();
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "pull".into(),
            device_path.clone(),
            recording.artifact_path.to_string_lossy().into_owned(),
        ])
        .await?;
        // The transferred MP4 is now durable on the host. Best-effort cleanup
        // avoids accumulating recordings on a physical device.
        let _ = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "rm".into(),
                "-f".into(),
                device_path,
            ])
            .await;
        Ok(recording.artifact_path.to_string_lossy().into_owned())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "exec-out".into(),
                "screencap".into(),
                "-p".into(),
            ])
            .await?;
        if output.stdout.is_empty() {
            return Err(RobotError::CommandFailed(
                "adb screencap returned an empty image".into(),
            ));
        }
        Ok(output.stdout)
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let x = android_coordinate(point.x, "x")?;
        let y = android_coordinate(point.y, "y")?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "tap".into(),
            x.to_string(),
            y.to_string(),
        ])
        .await?;
        Ok(())
    }

    async fn pointer_gesture(
        &self,
        device_id: &str,
        point: Point,
        gesture: robot_core::PointerGesture,
        context: Option<&robot_core::WebContext>,
    ) -> Result<(), RobotError> {
        use robot_core::PointerGesture::*;
        if gesture == Tap {
            return self.tap_in_context(device_id, point, context).await;
        }
        if context.is_some_and(|value| !value.is_empty()) {
            return Err(RobotError::Unsupported(
                "Android DOM pointer gestures".into(),
            ));
        }
        match gesture {
            Tap => unreachable!(),
            LongPress { duration_ms } => {
                self.swipe(device_id, point, point, Some(duration_ms)).await
            }
            DoubleTap => {
                ensure_device_id(device_id)?;
                let x = android_coordinate(point.x, "x")?;
                let y = android_coordinate(point.y, "y")?;
                // Use one shell and a short gap so Android recognizes two taps
                // without measuring the delay across separate ADB round trips.
                self.run_adb(&[
                    "-s".into(),
                    device_id.into(),
                    "shell".into(),
                    format!("input tap {x} {y} && sleep 0.07 && input tap {x} {y}"),
                ])
                .await
                .map(|_| ())
            }
        }
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        let resolved = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "cmd".into(),
                "package".into(),
                "resolve-activity".into(),
                "--brief".into(),
                "-a".into(),
                "android.intent.action.MAIN".into(),
                "-c".into(),
                "android.intent.category.LAUNCHER".into(),
                "-p".into(),
                app_id.into(),
            ])
            .await?;
        let text = String::from_utf8_lossy(&resolved.stdout);
        let component = text
            .lines()
            .map(str::trim)
            .find(|line| {
                line.split_once('/').is_some_and(|(package, activity)| {
                    package == app_id
                        && !activity.is_empty()
                        && activity
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._$".contains(&byte))
                })
            })
            .ok_or_else(|| {
                RobotError::CommandFailed(format!("No launcher activity for {app_id}"))
            })?;
        let launched = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "am".into(),
                "start".into(),
                "-a".into(),
                "android.intent.action.MAIN".into(),
                "-c".into(),
                "android.intent.category.LAUNCHER".into(),
                "-n".into(),
                format!("'{component}'"),
            ])
            .await?;
        let stdout = String::from_utf8_lossy(&launched.stdout);
        let stderr = String::from_utf8_lossy(&launched.stderr);
        if let Some(error) = stdout
            .lines()
            .chain(stderr.lines())
            .map(str::trim)
            .find(|line| line.starts_with("Error:") || line.starts_with("Error type "))
        {
            return Err(RobotError::CommandFailed(error.to_owned()));
        }
        Ok(())
    }

    async fn terminate_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "am".into(),
            "force-stop".into(),
            "--user".into(),
            "current".into(),
            app_id.into(),
        ])
        .await?;
        Ok(())
    }

    async fn terminate_all_apps(&self, device_id: &str) -> Result<Vec<String>, RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "pm".into(),
                "list".into(),
                "packages".into(),
                "-3".into(),
                "--user".into(),
                "current".into(),
            ])
            .await?;
        let mut packages = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            let package = line.trim().strip_prefix("package:").ok_or_else(|| {
                RobotError::CommandFailed("Unexpected Android package list; no apps stopped".into())
            })?;
            ensure_app_id(package)?;
            packages.push(package.to_owned());
        }
        packages.sort();
        packages.dedup();
        for (count, package) in packages.iter().enumerate() {
            self.terminate_app(device_id, package)
                .await
                .map_err(|error| {
                    RobotError::CommandFailed(format!(
                        "Stopped {count} apps; could not stop {package}: {error}"
                    ))
                })?;
        }
        Ok(packages)
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_web_url(url)?;
        if let Some(app_id) = app_id {
            ensure_app_id(app_id)?;
        }

        let mut args = vec![
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "am".into(),
            "start".into(),
            "-W".into(),
            "-a".into(),
            "android.intent.action.VIEW".into(),
            "-d".into(),
            url.into(),
        ];
        if let Some(app_id) = app_id {
            args.extend(["-p".into(), app_id.into()]);
        }
        self.run_adb(&args).await?;
        Ok(())
    }

    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        let output = self
            .run_adb(&[
                "-s".into(),
                id.into(),
                "shell".into(),
                "dumpsys".into(),
                "activity".into(),
                "activities".into(),
            ])
            .await?;
        let text = String::from_utf8_lossy(&output.stdout);
        let app = text
            .lines()
            .find(|line| line.contains("mResumedActivity") || line.contains("topResumedActivity"))
            .and_then(|line| line.split_whitespace().find(|part| part.contains('/')))
            .and_then(|part| part.split('/').next());
        Ok(json!({"appId":app}))
    }
    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let elements = self.list_elements(device_id).await?;
        let mut editors = elements.iter().filter(|element| {
            element.focused == Some(true)
                && element.enabled
                && element.element_type == Some(ElementType::TextField)
        });
        let editor = editors
            .next()
            .filter(|_| editors.next().is_none())
            .ok_or_else(|| {
                RobotError::InvalidInput(
                    "Focus an editable Android field before clearing text".into(),
                )
            })?;
        // Some Android editors handle Delete but ignore a keyboard Select All.
        // For a bounded visible value, remove text on both sides of the caret.
        const MAX_BATCH: usize = 512;
        if let Some(value) = editor.value.as_deref() {
            let count = value.chars().take(MAX_BATCH + 1).count();
            if (1..=MAX_BATCH).contains(&count) {
                let mut args = vec![
                    "-s".into(),
                    device_id.into(),
                    "shell".into(),
                    "input".into(),
                    "keyevent".into(),
                ];
                args.extend(
                    std::iter::repeat_n("KEYCODE_DEL", count)
                        .chain(std::iter::repeat_n("KEYCODE_FORWARD_DEL", count))
                        .map(str::to_owned),
                );
                return self.run_adb(&args).await.map(|_| ());
            }
        }
        let temporary = if self.video_controls.available(device_id).await {
            None
        } else {
            Some(video::start(self.adb_path(), device_id, &self.video_controls).await?)
        };
        let result = self.video_controls.clear_text(device_id).await;
        drop(temporary);
        result
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let encoded = encode_input_text(text)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "text".into(),
            encoded,
        ])
        .await?;
        Ok(())
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "dumpsys".into(),
                "input_method".into(),
            ])
            .await?;
        let dumpsys = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!(
                "dumpsys input_method returned invalid UTF-8: {error}"
            ))
        })?;
        let observed = parse_focused_ime_action(&dumpsys)?;
        if observed != action {
            return Err(RobotError::CommandFailed(format!(
                "focused editor IME action is {}, not {}; KEYCODE_ENTER was not sent",
                observed.as_str(),
                action.as_str()
            )));
        }
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "keyevent".into(),
            "66".into(),
        ])
        .await?;
        Ok(())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        let start_x = android_coordinate(start.x, "startX")?;
        let start_y = android_coordinate(start.y, "startY")?;
        let end_x = android_coordinate(end.x, "endX")?;
        let end_y = android_coordinate(end.y, "endY")?;
        let duration = duration_ms.unwrap_or(300);
        if !(1..=60_000).contains(&duration) {
            return Err(RobotError::InvalidInput(
                "durationMs must be between 1 and 60000".into(),
            ));
        }
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "input".into(),
            "swipe".into(),
            start_x.to_string(),
            start_y.to_string(),
            end_x.to_string(),
            end_y.to_string(),
            duration.to_string(),
        ])
        .await?;
        Ok(())
    }

    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_apk_path(apk_path)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "install".into(),
            "-r".into(),
            apk_path.into(),
        ])
        .await?;
        Ok(())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        ensure_device_id(device_id)?;
        ensure_app_id(app_id)?;
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "uninstall".into(),
            app_id.into(),
        ])
        .await?;
        Ok(())
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(robot_flutter_bridge::configured(device_id)
            .await
            .unwrap_or_default())
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        ensure_device_id(device_id)?;
        // Android allows one UiAutomation service per device. QC Live and flow
        // replay share this driver (including clones), so serialize the entire
        // dump/read/cleanup here rather than only gating the live HTTP route.
        let gate = {
            let mut gates = self.tree_dump_locks.lock().await;
            gates.entry(device_id.to_owned()).or_default().clone()
        };
        let _guard = gate.lock().await;
        let sequence = TREE_DUMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let remote_path = format!("/sdcard/mobile-mcp-{sequence}.xml");
        self.run_adb(&[
            "-s".into(),
            device_id.into(),
            "shell".into(),
            "uiautomator".into(),
            "dump".into(),
            "--compressed".into(),
            remote_path.clone(),
        ])
        .await?;

        let xml_output = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "exec-out".into(),
                "cat".into(),
                remote_path.clone(),
            ])
            .await;
        // Always attempt cleanup once the dump has been created; a cleanup
        // failure must not hide an otherwise usable accessibility snapshot.
        let _ = self
            .run_adb(&[
                "-s".into(),
                device_id.into(),
                "shell".into(),
                "rm".into(),
                "-f".into(),
                remote_path,
            ])
            .await;

        let xml = String::from_utf8(xml_output?.stdout).map_err(|error| {
            RobotError::CommandFailed(format!("uiautomator dump returned invalid UTF-8: {error}"))
        })?;
        parse_uiautomator_xml(&xml)
    }
}

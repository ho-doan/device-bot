//! Discovery includes ADB transports and configured, stopped Android AVDs.
use std::{collections::HashSet, path::Path, time::Duration};

use super::*;

pub(super) fn emulator_path(adb_path: &str) -> String {
    if let Ok(path) = std::env::var("ANDROID_EMULATOR_PATH") {
        return path;
    }
    let executable = if cfg!(windows) {
        "emulator.exe"
    } else {
        "emulator"
    };
    let sibling = Path::new(adb_path)
        .parent()
        .and_then(Path::parent)
        .map(|sdk| sdk.join("emulator").join(executable));
    if let Some(path) = sibling.filter(|path| path.is_file()) {
        return path.to_string_lossy().into_owned();
    }
    for variable in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(path) = std::env::var_os(variable)
            .map(PathBuf::from)
            .map(|sdk| sdk.join("emulator").join(executable))
            .filter(|path| path.is_file())
        {
            return path.to_string_lossy().into_owned();
        }
    }
    // GUI launches often have no Android SDK entries in PATH.
    if let Some(home) = std::env::var_os("HOME") {
        for suffix in ["Library/Android/sdk", "Android/Sdk"] {
            let path = PathBuf::from(&home)
                .join(suffix)
                .join("emulator")
                .join(executable);
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    executable.into()
}

fn avd_names(output: &[u8]) -> Vec<String> {
    let mut seen = HashSet::new();
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|line| valid_avd_name(line) && seen.insert((*line).to_owned()))
        .map(str::to_owned)
        .collect()
}

fn valid_avd_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !name.starts_with('-')
}

impl<R: CommandRunner> AndroidRobot<R> {
    async fn adb_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            self.run_adb(&["devices".into(), "-l".into()]),
        )
        .await
        .map_err(|_| RobotError::CommandFailed("adb devices timed out".into()))??;
        let text = String::from_utf8(output.stdout).map_err(|error| {
            RobotError::CommandFailed(format!("adb devices returned invalid UTF-8: {error}"))
        })?;
        Ok(parse_adb_devices(&text))
    }

    async fn configured_avds(&self) -> Result<Vec<String>, RobotError> {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            self.runner.run(&self.emulator_path, &["-list-avds".into()]),
        )
        .await
        .map_err(|_| RobotError::CommandFailed("emulator -list-avds timed out".into()))??;
        if !output.success {
            return Err(RobotError::CommandFailed(format!(
                "emulator -list-avds failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(avd_names(&output.stdout))
    }

    async fn running_avd_name(&self, serial: &str) -> Option<String> {
        if !serial.starts_with("emulator-") {
            return None;
        }
        let output = tokio::time::timeout(
            Duration::from_secs(2),
            self.run_adb(&[
                "-s".into(),
                serial.into(),
                "emu".into(),
                "avd".into(),
                "name".into(),
            ]),
        )
        .await
        .ok()?
        .ok()?;
        avd_names(&output.stdout)
            .into_iter()
            .find(|name| name != "OK")
    }

    pub(super) async fn discover_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        let mut devices = self.adb_devices().await?;
        // Missing emulator tooling must not hide real phones or running ADB devices.
        let avds = self.configured_avds().await.unwrap_or_default();
        if avds.is_empty() {
            return Ok(devices);
        }
        let mut running = HashSet::new();
        for device in &mut devices {
            if let Some(name) = self.running_avd_name(&device.id).await {
                device.name = name.clone();
                running.insert(name);
            }
        }
        devices.extend(
            avds.into_iter()
                .filter(|name| !running.contains(name))
                .map(|name| RobotDevice {
                    id: format!("avd:{name}"),
                    name,
                    platform: Platform::Android,
                    state: DeviceState::Offline,
                }),
        );
        Ok(devices)
    }

    pub(super) async fn boot_emulator(&self, id: &str) -> Result<(), RobotError> {
        let _guard = self.boot_lock.lock().await;
        let Some(name) = id.strip_prefix("avd:") else {
            if id.starts_with("emulator-") {
                return self.wait_for_android(id).await;
            }
            return Err(RobotError::Unsupported(
                "start is only available for Android AVDs".into(),
            ));
        };
        if !valid_avd_name(name) || !self.configured_avds().await?.iter().any(|avd| avd == name) {
            return Err(RobotError::DeviceNotFound(id.into()));
        }
        // A stale QC row or a repeated start must not launch a second AVD instance.
        for device in self.adb_devices().await? {
            if self.running_avd_name(&device.id).await.as_deref() == Some(name) {
                return self.wait_for_android(&device.id).await;
            }
        }
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create emulator log directory: {error}"
                ))
            })?;
        let log_path = self.artifact_dir.join(format!("emulator-{name}.log"));
        self.runner
            .spawn_background(
                &self.emulator_path,
                &["-avd".into(), name.into()],
                &log_path,
            )
            .await?;
        let wait = async {
            loop {
                for device in self.adb_devices().await? {
                    if self.running_avd_name(&device.id).await.as_deref() == Some(name) {
                        return self.wait_for_android(&device.id).await;
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(150), wait).await.map_err(|_| {
            RobotError::CommandFailed(format!("AVD {name} did not finish starting within 150 seconds; check {}. It may still be starting; refresh devices before retrying.", log_path.display()))
        })?
    }

    async fn wait_for_android(&self, serial: &str) -> Result<(), RobotError> {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                if let Ok(output) = self
                    .run_adb(&[
                        "-s".into(),
                        serial.into(),
                        "shell".into(),
                        "getprop".into(),
                        "sys.boot_completed".into(),
                    ])
                    .await
                {
                    if String::from_utf8_lossy(&output.stdout).trim() == "1" {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
        .await
        .map_err(|_| {
            RobotError::CommandFailed(format!(
                "Android emulator {serial} did not finish booting within 120 seconds"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{FakeRunner, successful};

    #[tokio::test]
    async fn discovers_stopped_avds_without_duplicating_running_emulators() {
        let runner = FakeRunner::new([
            successful(
                "List of devices attached\nemulator-5554 device model:Pixel\nphone unauthorized\n",
            ),
            successful("INFO | diagnostic line\nPixel_API_34\nTablet_API_35\nTablet_API_35\n"),
            successful("Pixel_API_34\nOK\n"),
        ]);
        let robot = AndroidRobot::with_runner("adb", runner).with_emulator_path("emulator");
        let devices = robot.list_devices().await.unwrap();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].id, "emulator-5554");
        assert_eq!(devices[0].name, "Pixel_API_34");
        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert_eq!(devices[2].id, "avd:Tablet_API_35");
        assert_eq!(devices[2].state, DeviceState::Offline);
    }

    #[tokio::test]
    async fn starts_avd_and_waits_for_adb_boot_completion() {
        let runner = FakeRunner::new([
            successful("Pixel_API_34\n"),
            successful("List of devices attached\n"),
            successful("List of devices attached\nemulator-5554 device\n"),
            successful("Pixel_API_34\nOK\n"),
            successful("1\n"),
        ]);
        let dir = std::env::temp_dir().join(format!("device-bot-boot-test-{}", std::process::id()));
        let robot = AndroidRobot::with_runner_and_artifact_dir("adb", &dir, runner)
            .with_emulator_path("emulator");
        robot.boot_device("avd:Pixel_API_34").await.unwrap();
        let calls = robot.runner.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|(exe, args)| exe == "emulator" && args == &["-avd", "Pixel_API_34"])
        );
        assert_eq!(
            calls.last().unwrap().1,
            [
                "-s",
                "emulator-5554",
                "shell",
                "getprop",
                "sys.boot_completed"
            ]
        );
        let _ = std::fs::remove_dir(dir);
    }

    #[tokio::test]
    async fn repeated_start_reuses_running_avd() {
        let runner = FakeRunner::new([
            successful("Pixel\n"),
            successful("List of devices attached\nemulator-5554 device\n"),
            successful("Pixel\nOK\n"),
            successful("1\n"),
        ]);
        let robot = AndroidRobot::with_runner("adb", runner);
        robot.boot_device("avd:Pixel").await.unwrap();
        assert!(
            !robot
                .runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(_, args)| args.first().is_some_and(|a| a == "-avd"))
        );
    }

    #[tokio::test]
    async fn invalid_or_unknown_avd_does_not_launch() {
        let robot = AndroidRobot::with_runner("adb", FakeRunner::new([successful("Pixel\n")]));
        assert!(robot.boot_device("avd:../../bad").await.is_err());
        assert!(robot.boot_device("avd:Missing").await.is_err());
    }

    #[tokio::test]
    async fn missing_emulator_tooling_keeps_adb_devices_visible() {
        let robot = AndroidRobot::with_runner(
            "adb",
            FakeRunner::new([successful(
                "List of devices attached\nphone device model:Phone\nemulator-5554 offline\n",
            )]),
        );
        let devices = robot.list_devices().await.unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "phone");
        assert_eq!(devices[1].id, "emulator-5554");
        assert_eq!(devices[1].state, DeviceState::Offline);
    }
}

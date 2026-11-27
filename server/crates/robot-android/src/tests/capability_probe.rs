use super::*;
#[test]
fn responsive_and_failed_probes_have_distinct_statuses() {
    assert_eq!(
        android_probe_readiness(true, "ready", "blocked").status,
        robot_core::CapabilityStatus::Available
    );
    assert_eq!(
        android_probe_readiness(false, "ready", "blocked").status,
        robot_core::CapabilityStatus::Blocked
    );
}
struct ProbeRunner {
    healthy: bool,
    calls: std::sync::Mutex<Vec<Vec<String>>>,
}
#[async_trait::async_trait]
impl CommandRunner for ProbeRunner {
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let text = args.join(" ");
        let output = if executable != "adb" {
            Vec::new()
        } else if text.starts_with("devices") {
            b"List of devices attached\nserial device\n".to_vec()
        } else if text.contains("uiautomator dump") {
            if self.healthy {
                b"<hierarchy/>".to_vec()
            } else {
                Vec::new()
            }
        } else if text.contains("screencap") {
            b"\x89PNG\r\n\x1a\n".to_vec()
        } else if text.contains("screenrecord") && !self.healthy {
            Vec::new()
        } else {
            b"ready".to_vec()
        };
        Ok(CommandOutput {
            success: true,
            exit_code: Some(0),
            stdout: output,
            stderr: b"secret from helper".to_vec(),
        })
    }
}
#[tokio::test]
async fn driver_probes_readiness_without_input_and_never_reports_helper_output() {
    for healthy in [true, false] {
        let runner = ProbeRunner {
            healthy,
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let robot = AndroidRobot::with_runner("adb", runner).with_emulator_path("emulator");
        let report = robot.capabilities("serial").await.unwrap();
        let expected = if healthy {
            robot_core::CapabilityStatus::Available
        } else {
            robot_core::CapabilityStatus::Blocked
        };
        assert_eq!(report.capabilities.input.status, expected);
        assert_eq!(report.capabilities.screen_recording.status, expected);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("secret from helper")
        );
        let calls = robot.runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|args| {
            args.iter()
                .any(|arg| ["tap", "launch", "recordVideo"].contains(&arg.as_str()))
        }));
    }
}

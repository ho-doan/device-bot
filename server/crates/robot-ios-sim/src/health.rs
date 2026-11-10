//! Read-only readiness checks before starting an iOS Simulator session.
use crate::IosSimulatorRobot;
use robot_core::health::{CheckStatus, HealthCheck, HealthReport, RecordingCheck, RecordingOwner};
use robot_core::{DeviceState, Robot, RobotError};
use std::{
    path::Path,
    process::Output,
    time::{Duration, Instant},
};
use tokio::{process::Command, time::timeout};

const DEADLINE: Duration = Duration::from_secs(8);

fn check(name: &str, status: CheckStatus, detail: impl Into<String>) -> HealthCheck {
    HealthCheck {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

impl IosSimulatorRobot {
    /// Does not record, launch apps, start XCTest, kill processes or reset devices.
    pub async fn check_health(&self, device_id: &str) -> Result<HealthReport, RobotError> {
        Self::valid_device_id(device_id)?;
        let start = Instant::now();
        let mut checks = Vec::new();
        let mut owners = Vec::new();
        let devices = timeout(DEADLINE, self.list_devices()).await;
        match devices {
            Ok(Ok(devices)) => {
                checks.push(check(
                    "simctl",
                    CheckStatus::Pass,
                    "CoreSimulator responded",
                ));
                match devices
                    .iter()
                    .find(|d| d.id.eq_ignore_ascii_case(device_id))
                {
                    Some(d) if d.state == DeviceState::Available => {
                        checks.push(check(
                            "device_booted",
                            CheckStatus::Pass,
                            "Target Simulator is booted",
                        ));
                    }
                    _ => {
                        checks.push(check(
                            "device_booted",
                            CheckStatus::Fail,
                            "Target Simulator is absent or not booted",
                        ));
                        return Ok(report(device_id, start, checks, owners));
                    }
                }
            }
            error => {
                checks.push(check(
                    "simctl",
                    CheckStatus::Fail,
                    match error {
                        Ok(Err(e)) => e.to_string(),
                        _ => "CoreSimulator did not respond within 8 seconds".into(),
                    },
                ));
                return Ok(report(device_id, start, checks, owners));
            }
        }

        let derived = Self::xctest_derived_data();
        let cache = robot_ios_real::runner_cache::RunnerCache {
            project: &self.xctest_project,
            derived_data: &derived,
            simulator: true,
        };
        let version = cache.version()?;
        // Independent probes run concurrently; none starts a recording or agent.
        let guest_args = ["simctl", "spawn", device_id, "/usr/bin/true"];
        let (guest, display, recording, agent) = tokio::join!(
            run_output(&self.xcrun_path, &guest_args, DEADLINE),
            display_probe(&self.xcrun_path, device_id),
            self.check_recording_session(device_id),
            robot_ios_real::health::agent_check_for(true, device_id, &version),
        );
        checks.push(command_result("device_heartbeat", guest, false));
        checks.push(command_result("display_response", display, true));
        let recording = recording?;
        checks.push(recording.check);
        owners.extend(recording.owners);
        checks.push(agent);
        checks.push(check(
            "xctest_assets",
            if self.xctest_project.join("project.pbxproj").is_file() {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "XCTest helper project must be present in the selected source/installed assets",
        ));
        checks.push(check(
            "xctest_products",
            if cache.is_current(&version) {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
            if cache.is_current(&version) {
                "Compiled XCTest runner cache matches the current source version (separate from live helper readiness)"
            } else {
                "XCTest runner products are missing; the UI session requires runner setup/build"
            },
        ));
        Ok(report(device_id, start, checks, owners))
    }

    /// Detects open recording files in SimRenderServer, including orphan sessions
    /// whose simctl process has already exited. This is not an atomic reservation.
    pub async fn check_recording_session(
        &self,
        device_id: &str,
    ) -> Result<RecordingCheck, RobotError> {
        Self::valid_device_id(device_id)?;
        if !cfg!(target_os = "macos") {
            return Ok(RecordingCheck {
                check: check(
                    "recording_session",
                    CheckStatus::Unknown,
                    "Simulator recording inspection requires macOS",
                ),
                owners: vec![],
            });
        }
        let result = run_output(
            "/usr/sbin/lsof",
            &["-nP", "-c", "SimRenderServer", "-F0pcftn"],
            DEADLINE,
        )
        .await;
        Ok(recording_result(device_id, result))
    }
}

fn report(
    device: &str,
    start: Instant,
    checks: Vec<HealthCheck>,
    owners: Vec<RecordingOwner>,
) -> HealthReport {
    HealthReport::from_checks(
        "ios_simulator",
        device,
        start.elapsed().as_millis(),
        checks,
        owners,
    )
}

async fn run_output(program: &str, args: &[&str], deadline: Duration) -> Result<Output, String> {
    timeout(
        deadline,
        Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| format!("{program} did not respond before its deadline"))?
    .map_err(|e| format!("cannot run {program}: {e}"))
}

// simctl writes screenshots atomically: neither "-" nor /dev/stdout is a
// supported stream destination. Keep the probe in a private temporary directory.
struct ProbeImage(std::path::PathBuf);
impl Drop for ProbeImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("screen.png"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

async fn display_probe(xcrun: &str, device: &str) -> Result<Output, String> {
    use std::io::Read;
    let sequence = crate::ARTIFACT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "device-bot-health-{}-{sequence}",
        std::process::id()
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&directory)
        .map_err(|e| format!("cannot create screenshot probe directory: {e}"))?;
    let temporary = ProbeImage(directory);
    let image = temporary.0.join("screen.png");
    let path = image.to_str().ok_or("screenshot probe path is not UTF-8")?;
    let mut output = run_output(
        xcrun,
        &["simctl", "io", device, "screenshot", "--type=png", path],
        DEADLINE,
    )
    .await?;
    if output.status.success() {
        let mut header = [0; 8];
        std::fs::File::open(&image)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|e| format!("cannot read screenshot probe PNG: {e}"))?;
        output.stdout = header.into();
    }
    Ok(output)
}

fn command_result(name: &str, result: Result<Output, String>, png: bool) -> HealthCheck {
    match result {
        Ok(output)
            if output.status.success()
                && (!png || output.stdout.starts_with(b"\x89PNG\r\n\x1a\n")) =>
        {
            check(
                name,
                CheckStatus::Pass,
                "Probe responded successfully; no image is saved",
            )
        }
        Ok(output) => check(
            name,
            CheckStatus::Fail,
            format!(
                "Probe failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(512)
                    .collect::<String>()
            ),
        ),
        Err(error) => check(name, CheckStatus::Fail, error),
    }
}

fn recording_result(device: &str, result: Result<Output, String>) -> RecordingCheck {
    let unknown = |detail| RecordingCheck {
        check: check("recording_session", CheckStatus::Unknown, detail),
        owners: vec![],
    };
    let output = match result {
        Ok(v) => v,
        Err(e) => return unknown(e),
    };
    // lsof exits 1 with no output when there are no matching processes/files.
    if !output.stderr.is_empty()
        || !(output.status.success() || output.status.code() == Some(1) && output.stdout.is_empty())
    {
        return unknown(format!(
            "Recording inspection incomplete ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(512)
                .collect::<String>()
        ));
    }
    let files = match parse_recordings(&output.stdout) {
        Ok(v) => v,
        Err(e) => return unknown(e),
    };
    let mut matched = Vec::new();
    let mut unattributed = Vec::new();
    for file in files {
        match recording_device(&file.path) {
            Some(id) if id.eq_ignore_ascii_case(device) => matched.push(file),
            Some(_) => {}
            None => unattributed.push(file),
        }
    }
    if !matched.is_empty() {
        RecordingCheck {
            check: check(
                "recording_session",
                CheckStatus::Fail,
                "Recorder is busy: an active or stale session holds this device's video file. Finish the owning session; if its owner is gone, recover only this Simulator before retrying",
            ),
            owners: matched,
        }
    } else if !unattributed.is_empty() {
        RecordingCheck {
            check: check(
                "recording_session",
                CheckStatus::Unknown,
                "Open Simulator video files cannot be attributed to a device; inspect their owners before starting another recording",
            ),
            owners: unattributed,
        }
    } else {
        RecordingCheck {
            check: check(
                "recording_session",
                CheckStatus::Pass,
                "No open Simulator recording detected; this read-only check does not reserve the recorder",
            ),
            owners: vec![],
        }
    }
}

fn recording_device(path: &str) -> Option<&str> {
    let path = path.strip_suffix(" (deleted)").unwrap_or(path);
    let name = Path::new(path)
        .file_name()?
        .to_str()?
        .strip_prefix("mobile-mcp-ios-recording-")?;
    let id = name.get(..36)?;
    if name.get(36..37)? != "-"
        || id.bytes().enumerate().any(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c != b'-'
            } else {
                !c.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    Some(id)
}

fn session_pid(path: &str) -> Option<u32> {
    recording_device(path)?;
    let path = path.strip_suffix(" (deleted)").unwrap_or(path);
    let name = Path::new(path)
        .file_name()?
        .to_str()?
        .strip_prefix("mobile-mcp-ios-recording-")?;
    let pid = name.get(37..)?.split('-').next()?.parse::<u32>().ok()?;
    (pid > 0 && pid <= i32::MAX as u32).then_some(pid)
}

fn process_running(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        // Read-only existence probe. An alive PID can have been reused, so this
        // is evidence for manual diagnosis, never authority to kill/reboot.
        if unsafe { libc::kill(pid as i32, 0) } == 0 {
            return Some(true);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(false),
            Some(libc::EPERM) => Some(true),
            _ => None,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

fn parse_recordings(bytes: &[u8]) -> Result<Vec<RecordingOwner>, String> {
    let mut pid = None;
    let mut process = String::new();
    let mut file_type = String::new();
    let mut files = Vec::new();
    for field in bytes.split(|c| *c == 0) {
        let field = std::str::from_utf8(field)
            .map_err(|_| "lsof returned invalid UTF-8")?
            .trim_start_matches('\n');
        if field.is_empty() {
            continue;
        }
        let tag = field.get(..1).ok_or("Invalid lsof field prefix")?;
        let value = field.get(1..).ok_or("Invalid lsof field prefix")?;
        match tag {
            "p" => {
                pid = Some(value.parse::<u32>().map_err(|_| "invalid lsof PID")?);
                process.clear();
                file_type.clear();
            }
            "c" => process = value.into(),
            "f" => file_type.clear(),
            "t" => file_type = value.into(),
            "n" if {
                let name = value
                    .strip_suffix(" (deleted)")
                    .unwrap_or(value)
                    .to_ascii_lowercase();
                name.ends_with(".mp4") || name.ends_with(".mov")
            } =>
            {
                if file_type != "REG" || process.is_empty() {
                    return Err("Incomplete recording owner information".into());
                }
                files.push(RecordingOwner {
                    pid: pid.ok_or("Recording owner PID missing")?,
                    process: process.clone(),
                    path: value.into(),
                    session_process_id: session_pid(value),
                    session_process_running: session_pid(value).and_then(process_running),
                });
            }
            _ => {}
        }
    }
    if !bytes.is_empty() && pid.is_none() {
        return Err("lsof returned no process records".into());
    }
    Ok(files)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    const DEVICE: &str = "8F30E506-B952-48EB-A1DB-8D61701356E0";
    fn output(code: i32, bytes: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: bytes.into(),
            stderr: stderr.into(),
        }
    }
    fn listing(device: &str) -> Vec<u8> {
        format!("p33403\0cSimRenderServer\0\nf9\0tREG\0n/tmp/case with spaces/mobile-mcp-ios-recording-{device}-34288-0.mp4\0\n").into_bytes()
    }
    #[test]
    fn finds_service_owned_recording_even_without_a_simctl_process() {
        let result = recording_result(DEVICE, Ok(output(0, &listing(DEVICE), b"")));
        assert_eq!(result.check.status, CheckStatus::Fail);
        assert_eq!(result.owners[0].pid, 33403);
        assert!(result.owners[0].path.contains("case with spaces"));
    }
    #[test]
    fn no_matching_process_is_clear_but_errors_and_unknown_owners_are_not() {
        assert_eq!(
            recording_result(DEVICE, Ok(output(1, b"", b"")))
                .check
                .status,
            CheckStatus::Pass
        );
        for result in [
            recording_result(DEVICE, Ok(output(1, b"", b"permission denied"))),
            recording_result(DEVICE, Err("lsof timeout".into())),
            recording_result(DEVICE, Ok(output(0, b"invalid output", b""))),
            recording_result(
                DEVICE,
                Ok(output(
                    0,
                    b"p10\0cSimRenderServer\0f9\0tREG\0n/tmp/unattributed.mov\0",
                    b"",
                )),
            ),
        ] {
            assert_eq!(result.check.status, CheckStatus::Unknown);
        }
    }
    #[test]
    fn another_identified_device_does_not_block_this_device() {
        let bytes = listing("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        assert_eq!(
            recording_result(DEVICE, Ok(output(0, &bytes, b"")))
                .check
                .status,
            CheckStatus::Pass
        );
    }
    #[test]
    fn deleted_open_video_and_malformed_fields_do_not_look_clear() {
        let mut bytes = listing(DEVICE);
        let position = bytes.windows(4).position(|w| w == b".mp4").unwrap() + 4;
        bytes.splice(position..position, b" (deleted)".iter().copied());
        assert_eq!(
            recording_result(DEVICE, Ok(output(0, &bytes, b"")))
                .check
                .status,
            CheckStatus::Fail
        );
        for bytes in [
            b"\xff".as_slice(),
            "é".as_bytes(),
            b"pnot-a-pid\0",
            b"p12\0n/tmp/file.mp4\0",
        ] {
            assert!(parse_recordings(bytes).is_err());
        }
    }
    #[tokio::test]
    async fn command_deadlines_detect_hangs_and_invalid_images_fail() {
        assert!(
            run_output("/bin/sleep", &["2"], Duration::from_millis(10))
                .await
                .is_err()
        );
        assert_eq!(
            command_result("display_response", Ok(output(0, b"not png", b"")), true).status,
            CheckStatus::Fail
        );
    }
}

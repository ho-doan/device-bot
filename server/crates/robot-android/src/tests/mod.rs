use std::{collections::VecDeque, sync::Mutex};

use super::*;

#[cfg(windows)]
#[tokio::test]
async fn discovery_helper_does_not_create_a_windows_console() {
    let args = [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Add-Type 'using System; using System.Runtime.InteropServices; public class AdbConsoleProbe { [DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; [Console]::Write([AdbConsoleProbe]::GetConsoleWindow().ToInt64())",
    ].map(String::from);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        TokioCommandRunner.run("powershell.exe", &args),
    )
    .await
    .expect("console probe timed out")
    .expect("console probe could not start");
    assert!(
        output.success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[derive(Debug)]
pub(super) struct FakeRunner {
    outputs: Mutex<VecDeque<CommandOutput>>,
    pub(super) calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl FakeRunner {
    pub(super) fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
        Self {
            outputs: Mutex::new(outputs.into_iter().collect()),
            calls: Mutex::new(vec![]),
        }
    }
}

#[async_trait]
impl CommandRunner for FakeRunner {
    async fn spawn_background(
        &self,
        executable: &str,
        args: &[String],
        _: &std::path::Path,
    ) -> Result<(), RobotError> {
        self.calls
            .lock()
            .unwrap()
            .push((executable.into(), args.to_vec()));
        Ok(())
    }
    async fn run(&self, executable: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        self.calls
            .lock()
            .unwrap()
            .push((executable.to_owned(), args.to_vec()));
        self.outputs
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| RobotError::CommandFailed("fake queue exhausted".into()))
    }
}

pub(super) fn successful(stdout: impl Into<Vec<u8>>) -> CommandOutput {
    CommandOutput {
        success: true,
        exit_code: Some(0),
        stdout: stdout.into(),
        stderr: vec![],
    }
}

// Android permits only one UiAutomation registration per device. Model the
// real exit 137 when QC inspection and flow replay dump the same tree at once.
#[derive(Debug, Default, Clone)]
struct ConcurrentTreeRunner {
    active: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
}

#[async_trait]
impl CommandRunner for ConcurrentTreeRunner {
    async fn run(&self, _: &str, args: &[String]) -> Result<CommandOutput, RobotError> {
        if args.get(3).is_some_and(|arg| arg == "uiautomator") {
            let device = args[1].clone();
            if !self.active.lock().unwrap().insert(device.clone()) {
                return Ok(CommandOutput {
                    success: false,
                    exit_code: Some(137),
                    stdout: vec![],
                    stderr: vec![],
                });
            }
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            } else {
                tokio::task::yield_now().await;
            }
            self.active.lock().unwrap().remove(&device);
        }
        if args.get(2).is_some_and(|arg| arg == "exec-out") {
            return Ok(successful(
                r#"<hierarchy><node text="Import video" class="android.widget.Button" clickable="true" enabled="true" bounds="[0,0][100,40]" /></hierarchy>"#,
            ));
        }
        Ok(successful(""))
    }
}

#[tokio::test]
async fn stationary_press_and_double_tap_use_single_android_shell_operations() {
    let robot = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([successful(""), successful("")]),
    );
    let at = Point { x: 24., y: 42. };
    robot
        .pointer_gesture(
            "phone",
            at,
            robot_core::PointerGesture::LongPress { duration_ms: 845 },
            None,
        )
        .await
        .unwrap();
    robot
        .pointer_gesture("phone", at, robot_core::PointerGesture::DoubleTap, None)
        .await
        .unwrap();
    let calls = robot.runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].1,
        [
            "-s", "phone", "shell", "input", "swipe", "24", "42", "24", "42", "845"
        ]
    );
    assert_eq!(
        calls[1].1,
        [
            "-s",
            "phone",
            "shell",
            "input tap 24 42 && sleep 0.07 && input tap 24 42",
        ]
    );
}

#[tokio::test]
async fn concurrent_tree_reads_on_one_device_do_not_kill_uiautomator() {
    let robot = AndroidRobot::with_runner("adb-test", ConcurrentTreeRunner::default());
    let replay = robot.clone();
    let (live, flow) = tokio::join!(robot.list_elements("phone"), replay.list_elements("phone"),);
    assert_eq!(live.unwrap().len(), 1);
    assert_eq!(flow.unwrap().len(), 1);
}

#[tokio::test]
async fn tree_reads_on_different_devices_can_run_concurrently() {
    let runner = ConcurrentTreeRunner {
        barrier: Some(Arc::new(tokio::sync::Barrier::new(2))),
        ..Default::default()
    };
    let robot = AndroidRobot::with_runner("adb-test", runner);
    let reads = async { tokio::join!(robot.list_elements("first"), robot.list_elements("second")) };
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(1), reads)
        .await
        .expect("different devices must not share a dump lock");
    assert_eq!(first.unwrap().len(), 1);
    assert_eq!(second.unwrap().len(), 1);
}

#[test]
fn parses_all_adb_device_states() {
    let devices = parse_adb_devices(
        "List of devices attached\n\
         emulator-5554 device product:sdk_gphone64_arm64 model:Pixel_9 device:emu64a\n\
         R5CT30ABCD unauthorized transport_id:1\n\
         tcp:192.168.1.2:5555 offline product:foo\n",
    );

    assert_eq!(devices.len(), 3);
    assert_eq!(devices[0].name, "Pixel 9");
    assert_eq!(devices[0].state, DeviceState::Available);
    assert_eq!(devices[1].state, DeviceState::Unauthorized);
    assert_eq!(devices[2].state, DeviceState::Offline);
}

#[test]
fn parses_installed_packages_with_apk_paths() {
    let apps = parse_adb_installed_apps(
        "package:/data/app/~~abc==/com.example.one-xyz==/base.apk=com.example.one\n\
         package:/system/priv-app/Settings/Settings.apk=com.android.settings\n",
    );

    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["packageName"], "com.example.one");
    assert_eq!(
        apps[1]["sourcePath"],
        "/system/priv-app/Settings/Settings.apk"
    );
}

#[test]
fn parses_uiautomator_nodes_without_losing_accessibility_metadata() {
    let elements = parse_uiautomator_xml(
        r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
        <hierarchy rotation="0">
          <node index="0" text="Search" resource-id="com.android.chrome:id/search_box" class="android.widget.EditText" package="com.android.chrome" content-desc="Search query" clickable="true" enabled="true" bounds="[10,20][300,80]" />
        </hierarchy>"#,
    )
    .unwrap();

    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].text.as_deref(), Some("Search"));
    assert_eq!(
        elements[0].resource_id.as_deref(),
        Some("com.android.chrome:id/search_box")
    );
    assert_eq!(
        elements[0].bounds,
        Bounds {
            left: 10,
            top: 20,
            right: 300,
            bottom: 80,
        }
    );
    assert!(elements[0].clickable);
    assert_eq!(elements[0].focused, None);
    assert_eq!(elements[0].checked, None);
    assert_eq!(elements[0].element_type, Some(ElementType::TextField));
    assert_eq!(elements[0].value.as_deref(), Some("Search"));
}

#[tokio::test]
async fn type_filter_keeps_a_password_field_that_follows_other_controls() {
    let xml = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
        <hierarchy>
          <node text="Save" class="android.widget.Button" clickable="true" enabled="true" bounds="[0,0][10,10]" />
          <node text="" class="android.widget.EditText" password="true" resource-id="password" focused="true" bounds="[0,20][10,40]" />
          <node text="user@example.com" class="android.widget.EditText" selected="false" bounds="[0,50][10,70]" />
        </hierarchy>"#;
    let robot = AndroidRobot::with_runner(
        "adb",
        FakeRunner::new((0..2).flat_map(|_| [successful(""), successful(xml), successful("")])),
    );
    let query = robot_core::ElementQuery {
        element_type: Some(ElementType::TextField),
        max_results: Some(1),
        ..Default::default()
    };
    let found = robot
        .list_elements_query("emulator-5554", query.clone())
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].resource_id.as_deref(), Some("password"));
    assert_eq!(found[0].focused, Some(true));
    assert_eq!(found[0].value, None);

    let email = robot
        .list_elements_query(
            "emulator-5554",
            robot_core::ElementQuery {
                filter: Some("USER@example.com".into()),
                ..query
            },
        )
        .await
        .unwrap();
    assert_eq!(email.len(), 1);
    assert_eq!(email[0].text.as_deref(), Some("user@example.com"));
    assert_eq!(email[0].selected, Some(false));
    assert_eq!(email[0].checked, None);
}

#[test]
fn reads_the_focused_editor_action_and_rejects_a_mismatch() {
    let next = "mServedView=android.view.View\n    imeOptions=0x5\n";
    assert_eq!(parse_focused_ime_action(next).unwrap(), ImeAction::Next);
    let done = "mCurrentTextBoxAttribute: EditorInfo{imeOptions=0x10000006}";
    assert_eq!(parse_focused_ime_action(done).unwrap(), ImeAction::Done);
    assert!(
        parse_focused_ime_action("mServedView=null\nimeOptions=6\n")
            .unwrap_err()
            .to_string()
            .contains("no focused editor")
    );
    assert!(
        parse_focused_ime_action("mServedView=android.view.View\nimeOptions=1\n")
            .unwrap_err()
            .to_string()
            .contains("KEYCODE_ENTER was not sent")
    );
}

#[tokio::test]
async fn clear_rejects_non_editable_focus_before_sending_keys() {
    let runner = FakeRunner::new([
        successful(""),
        successful(
            r#"<hierarchy><node text="Button" class="android.widget.Button" focused="true" enabled="true" bounds="[0,0][100,40]" /></hierarchy>"#,
        ),
        successful(""),
    ]);
    let robot = AndroidRobot::with_runner("adb-test", runner);
    let error = robot.clear_text("phone").await.unwrap_err();
    assert!(error.to_string().contains("Focus an editable"));
    assert_eq!(robot.runner.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn clears_observed_text_on_both_sides_of_the_caret() {
    let robot = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([
            successful(""),
            successful(
                r#"<hierarchy><node text="aé" class="android.widget.EditText" focused="true" enabled="true" bounds="[0,0][100,40]" /></hierarchy>"#,
            ),
            successful(""),
            successful(""),
        ]),
    );

    robot.clear_text("phone").await.unwrap();

    let calls = robot.runner.calls.lock().unwrap();
    assert_eq!(
        calls[3].1[2..],
        [
            "shell",
            "input",
            "keyevent",
            "KEYCODE_DEL",
            "KEYCODE_DEL",
            "KEYCODE_FORWARD_DEL",
            "KEYCODE_FORWARD_DEL"
        ]
    );
}

#[tokio::test]
async fn sends_enter_only_when_the_focused_ime_action_matches() {
    let runner = FakeRunner::new([
        successful("mServedView=android.widget.EditText\nimeOptions=0x10000005\n"),
        successful(vec![]),
    ]);
    let robot = AndroidRobot::with_runner("adb-test", runner);
    robot
        .press_ime_action("emulator-5554", ImeAction::Next)
        .await
        .unwrap();
    {
        let calls = robot.runner.calls.lock().unwrap();
        assert_eq!(calls[0].1[2..], ["shell", "dumpsys", "input_method"]);
        assert_eq!(calls[1].1[2..], ["shell", "input", "keyevent", "66"]);
    }

    let mismatch = FakeRunner::new([successful(
        "mServedView=android.widget.EditText\nimeOptions=5\n",
    )]);
    let robot = AndroidRobot::with_runner("adb-test", mismatch);
    let error = robot
        .press_ime_action("emulator-5554", ImeAction::Done)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not done"));
    assert!(error.contains("KEYCODE_ENTER was not sent"));
    assert_eq!(robot.runner.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn uses_exec_out_for_screenshots_and_shell_input_for_taps() {
    let runner = FakeRunner::new([successful(b"png".to_vec()), successful(vec![])]);
    let robot = AndroidRobot::with_runner("adb-test", runner);

    assert_eq!(
        robot.take_screenshot("emulator-5554").await.unwrap(),
        b"png"
    );
    robot
        .tap("emulator-5554", Point { x: 12.5, y: 99.6 })
        .await
        .unwrap();

    assert_eq!(
        robot.runner.calls.lock().unwrap().as_slice(),
        [
            (
                "adb-test".to_owned(),
                vec!["-s", "emulator-5554", "exec-out", "screencap", "-p"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
            (
                "adb-test".to_owned(),
                vec!["-s", "emulator-5554", "shell", "input", "tap", "13", "100"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
        ]
    );
}

#[tokio::test]
async fn lists_devices_through_adb() {
    let runner = FakeRunner::new([successful(
        "List of devices attached\nemulator-5554 device model:Pixel_9\n",
    )]);
    let robot = AndroidRobot::with_runner("adb", runner);

    let devices = robot.list_devices().await.unwrap();
    assert_eq!(devices[0].id, "emulator-5554");
    assert_eq!(
        robot.runner.calls.lock().unwrap()[0].1,
        vec!["devices", "-l"]
    );
}

#[tokio::test]
async fn terminates_apps_for_current_user_and_rejects_invalid_packages() {
    let robot = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([
            successful(""),
            successful(
                "package:com.example.two\npackage:com.example.one\npackage:com.example.one\n",
            ),
            successful(""),
            successful(""),
        ]),
    );
    robot
        .terminate_app("emulator-5554", "com.example.focused")
        .await
        .unwrap();
    let apps = robot.terminate_all_apps("emulator-5554").await.unwrap();
    assert_eq!(apps, ["com.example.one", "com.example.two"]);
    let calls = robot.runner.calls.lock().unwrap().clone();
    assert_eq!(
        calls[0].1,
        [
            "-s",
            "emulator-5554",
            "shell",
            "am",
            "force-stop",
            "--user",
            "current",
            "com.example.focused"
        ]
    );
    assert_eq!(
        calls[1].1,
        [
            "-s",
            "emulator-5554",
            "shell",
            "pm",
            "list",
            "packages",
            "-3",
            "--user",
            "current"
        ]
    );
    assert_eq!(calls[2].1.last().unwrap(), "com.example.one");
    drop(calls);
    assert!(
        robot
            .terminate_app("emulator-5554", "com.app;reboot")
            .await
            .is_err()
    );
    assert_eq!(robot.runner.calls.lock().unwrap().len(), 4);
    let malformed = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([successful("package:com.good.app\npackage:bad;reboot\n")]),
    );
    assert!(malformed.terminate_all_apps("emulator-5554").await.is_err());
    assert_eq!(malformed.runner.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn reports_partial_app_termination_failure() {
    let robot = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([
            successful("package:com.example.one\npackage:com.example.two\n"),
            successful(""),
            CommandOutput {
                success: false,
                exit_code: Some(1),
                stdout: vec![],
                stderr: b"denied".to_vec(),
            },
        ]),
    );
    let error = robot
        .terminate_all_apps("emulator-5554")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Stopped 1 apps"), "{error}");
    assert!(error.contains("com.example.two"), "{error}");
}

#[tokio::test]
async fn uses_explicit_adb_commands_for_app_navigation_and_text() {
    let runner = FakeRunner::new([
        successful("priority=0 match=0x108000\ncom.android.chrome/.Main\n"),
        successful("Starting: Intent { cmp=com.android.chrome/.Main }"),
        successful(""),
        successful(""),
    ]);
    let robot = AndroidRobot::with_runner("adb-test", runner);

    robot
        .launch_app("emulator-5554", "com.android.chrome")
        .await
        .unwrap();
    robot
        .open_url(
            "emulator-5554",
            "https://www.youtube.com/results?search_query=babyshake",
            Some("com.android.chrome"),
        )
        .await
        .unwrap();
    robot
        .type_text("emulator-5554", "baby shake")
        .await
        .unwrap();

    assert_eq!(
        robot.runner.calls.lock().unwrap().as_slice(),
        [
            (
                "adb-test".to_owned(),
                vec![
                    "-s",
                    "emulator-5554",
                    "shell",
                    "cmd",
                    "package",
                    "resolve-activity",
                    "--brief",
                    "-a",
                    "android.intent.action.MAIN",
                    "-c",
                    "android.intent.category.LAUNCHER",
                    "-p",
                    "com.android.chrome"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
            (
                "adb-test".to_owned(),
                vec![
                    "-s",
                    "emulator-5554",
                    "shell",
                    "am",
                    "start",
                    "-a",
                    "android.intent.action.MAIN",
                    "-c",
                    "android.intent.category.LAUNCHER",
                    "-n",
                    "'com.android.chrome/.Main'"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
            (
                "adb-test".to_owned(),
                vec![
                    "-s",
                    "emulator-5554",
                    "shell",
                    "am",
                    "start",
                    "-W",
                    "-a",
                    "android.intent.action.VIEW",
                    "-d",
                    "https://www.youtube.com/results?search_query=babyshake",
                    "-p",
                    "com.android.chrome"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
            (
                "adb-test".to_owned(),
                vec![
                    "-s",
                    "emulator-5554",
                    "shell",
                    "input",
                    "text",
                    "baby%sshake"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
        ]
    );
}

#[tokio::test]
async fn launch_rejects_missing_launcher_activity_even_if_adb_succeeds() {
    let robot = AndroidRobot::with_runner(
        "adb-test",
        FakeRunner::new([successful("No activity found")]),
    );

    let error = robot
        .launch_app("emulator-5554", "com.example.orbit")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("No launcher activity"), "{error}");
    assert_eq!(robot.runner.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn records_with_a_device_pid_and_pulls_the_final_mp4() {
    let runner = FakeRunner::new([
        successful(vec![]),
        successful("4242\n"),
        successful(vec![]),
        successful(vec![]),
        successful(vec![]),
    ]);
    let artifact_dir = std::env::temp_dir().join("robot-android-recording-test");
    let robot = AndroidRobot::with_runner_and_artifact_dir("adb-test", &artifact_dir, runner);

    let started = robot.start_screen_recording("emulator-5554").await.unwrap();
    let stopped = robot.stop_screen_recording("emulator-5554").await.unwrap();

    assert_eq!(started, stopped);
    assert!(stopped.ends_with(".mp4"));
    let calls = robot.runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[0].1[2..5], ["shell", "rm", "-f"]);
    assert_eq!(calls[1].1[2], "shell");
    assert!(calls[1].1[3].starts_with("setsid screenrecord --time-limit 1800 /sdcard/"));
    assert_eq!(calls[2].1[2], "shell");
    assert!(calls[2].1[3].starts_with("kill -INT 4242"));
    assert_eq!(calls[3].1[2], "pull");
    assert_eq!(calls[3].1[4], stopped);
    assert_eq!(calls[4].1[2..5], ["shell", "rm", "-f"]);
}

use robot_core::{ElementQuery, Robot, RobotError, WebContext};

#[tokio::test]
async fn adapters_reject_unsupported_query_options_before_native_io() {
    let adapters: [(&str, &str, Box<dyn Robot>); 5] = [
        (
            "Android",
            "unavailable-android",
            Box::new(robot_android::AndroidRobot::new("unused-adb", ".")),
        ),
        (
            "iOS device",
            "unavailable-ios",
            Box::new(robot_ios_real::IosXCTestRobot::new(
                "unused-xcrun",
                robot_ios_real::XCTestAgentBridge::new(None),
            )),
        ),
        (
            "macOS",
            "macos-host",
            Box::new(robot_macos::MacosRobot::new(".")),
        ),
        (
            "Windows",
            "windows-host",
            Box::new(robot_windows::WindowsRobot::new(".")),
        ),
        (
            "Chrome",
            "chrome:unavailable",
            Box::new(robot_chrome::ChromeRobot::new("127.0.0.1:0")),
        ),
    ];

    let query = ElementQuery {
        exclude_system_keyboard: true,
        ..Default::default()
    };
    for (name, device, robot) in adapters {
        let result = robot.list_elements_query(device, query.clone()).await;
        assert!(
            matches!(&result, Err(RobotError::Unsupported(message)) if message.contains("keyboard exclusion")),
            "{name} must report unsupported keyboard exclusion, got {result:?}"
        );

        if matches!(name, "Android" | "macOS" | "Windows") {
            let scoped = ElementQuery {
                web_context: Some(WebContext {
                    app_id: Some("fixture.app".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let result = robot.list_elements_query(device, scoped).await;
            assert!(
                matches!(&result, Err(RobotError::Unsupported(message)) if message.contains("web context")),
                "{name} must report unsupported document scope, got {result:?}"
            );
        }
    }
}

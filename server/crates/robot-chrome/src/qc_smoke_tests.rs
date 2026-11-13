use super::*;

#[tokio::test]
#[ignore = "requires Chrome, and QC web fixture with notes-web-verification-token at :3349"]
async fn qc_web_pointer_and_keyboard_update_the_real_controller() {
    let mut robot = ChromeRobot::from_env();
    robot.artifact_dir = std::env::temp_dir().join("device-bot-qc-browser-smoke");
    robot.boot_device("chrome:browser").await.unwrap();
    let id = robot.list_devices().await.unwrap()[0].id.clone();
    let result = async {
        robot
            .open_url(&id, "http://127.0.0.1:3349/qc/", None)
            .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let (field, button) = loop {
            let elements = robot.list_elements(&id).await?;
            let fields: Vec<_> = elements
                .iter()
                .filter(|e| {
                    e.class_name == "input"
                        && e.content_description.as_deref() == Some("Bearer token")
                })
                .collect();
            let field = (fields.len() == 1).then(|| fields[0]);
            let button = elements
                .iter()
                .find(|e| e.text.as_deref() == Some("Refresh") && e.clickable);
            if let (Some(field), Some(button)) = (field, button) {
                break (field.clone(), button.clone());
            }
            if tokio::time::Instant::now() > deadline {
                return Err(RobotError::CommandFailed("QC fields did not render".into()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let center = |e: &UiElement| Point {
            x: (e.bounds.left + e.bounds.right) as f64 / 2.,
            y: (e.bounds.top + e.bounds.bottom) as f64 / 2.,
        };
        robot.tap(&id, center(&field)).await?;
        robot.type_text(&id, "notes-web-verification-token").await?;
        robot.tap(&id, center(&button)).await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let elements = robot.list_elements(&id).await?;
            if elements
                .iter()
                .any(|e| e.text.as_deref() == Some("MCP session connected."))
            {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                return Err(RobotError::CommandFailed(
                    "QC stayed disconnected after real pointer/keyboard input".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok::<(), RobotError>(())
    }
    .await;
    robot.close_session().await.unwrap();
    result.unwrap();
}

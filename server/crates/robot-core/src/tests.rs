//! Contract and routing regressions exercised through the public interface.

use super::*;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

struct StubRobot {
    device: Option<RobotDevice>,
    list_error: bool,
}

#[async_trait]
impl Robot for StubRobot {
    async fn type_text_in_context(
        &self,
        id: &str,
        _: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        Err(RobotError::CommandFailed(format!(
            "forwarded:{id}:{}",
            context.unwrap().app_id.as_deref().unwrap()
        )))
    }
    async fn clear_text_in_context(
        &self,
        id: &str,
        context: Option<&WebContext>,
    ) -> Result<(), RobotError> {
        self.type_text_in_context(id, "", context).await
    }
    async fn stream_context(&self, id: &str) -> Result<Value, RobotError> {
        Ok(serde_json::json!({ "deviceId": id }))
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        if self.list_error {
            return Err(RobotError::CommandFailed("driver unavailable".into()));
        }
        Ok(self.device.clone().into_iter().collect())
    }

    async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
        Ok(vec![7])
    }

    async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
        Ok(())
    }

    async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }

    async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
        Ok(())
    }

    async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }

    async fn swipe(&self, _: &str, _: Point, _: Point, _: Option<u64>) -> Result<(), RobotError> {
        Ok(())
    }

    async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }

    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }

    async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(vec![])
    }
}

fn element(class_name: &str, text: &str) -> UiElement {
    UiElement {
        class_name: class_name.into(),
        resource_id: None,
        text: Some(text.into()),
        content_description: None,
        bounds: Bounds {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        },
        clickable: true,
        enabled: true,
        focused: None,
        checked: None,
        selected: None,
        value: None,
        element_type: None,
        web_context: None,
    }
}

struct TreeRobot {
    elements: Vec<UiElement>,
}

#[async_trait]
impl Robot for TreeRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![])
    }
    async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
        Ok(vec![])
    }
    async fn tap(&self, _: &str, _: Point) -> Result<(), RobotError> {
        Ok(())
    }
    async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }
    async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
        Ok(())
    }
    async fn type_text(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }
    async fn swipe(&self, _: &str, _: Point, _: Point, _: Option<u64>) -> Result<(), RobotError> {
        Ok(())
    }
    async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }
    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Ok(())
    }
    async fn list_elements(&self, _: &str) -> Result<Vec<UiElement>, RobotError> {
        Ok(self.elements.clone())
    }
}

#[tokio::test]
async fn element_type_and_text_filter_run_before_max_results() {
    let robot = TreeRobot {
        elements: vec![
            element("android.widget.Button", "Save"),
            element("android.widget.Button", "Cancel"),
            element("android.widget.EditText", "secret"),
            element("android.widget.EditText", "user@example.com"),
        ],
    };

    let first_text_field = robot
        .list_elements_query(
            "device",
            ElementQuery {
                filter: None,
                element_type: Some(ElementType::TextField),
                max_results: Some(1),
                web_context: None,
                include_frames: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(first_text_field.len(), 1);
    assert_eq!(first_text_field[0].text.as_deref(), Some("secret"));
    assert_eq!(
        first_text_field[0].element_type,
        Some(ElementType::TextField)
    );

    let email = robot
        .list_elements_query(
            "device",
            ElementQuery {
                filter: Some("user@example.com".into()),
                element_type: Some(ElementType::TextField),
                max_results: Some(2),
                web_context: None,
                include_frames: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(email.len(), 1);
    assert_eq!(email[0].text.as_deref(), Some("user@example.com"));

    let untyped = robot
        .list_elements_filtered("device", None, Some(1))
        .await
        .unwrap();
    assert_eq!(untyped[0].text.as_deref(), Some("Save"));
}

#[test]
fn classifies_password_editors_as_text_fields_and_rejects_unknown_types() {
    assert_eq!(
        classify_class_name("android.widget.EditText"),
        Some(ElementType::TextField)
    );
    assert_eq!(
        classify_class_name("androidx.appcompat.widget.AppCompatEditText"),
        Some(ElementType::TextField)
    );
    assert_eq!(
        classify_class_name("android.widget.ImageButton"),
        Some(ElementType::Button)
    );
    assert!(ElementType::parse("EditText").is_err());
    assert!(
        ElementType::parse("not-a-type")
            .unwrap_err()
            .to_string()
            .contains("unsupported elementType")
    );
    assert_eq!(ImeAction::from_android_action_id(5), Some(ImeAction::Next));
    assert_eq!(
        ImeAction::from_android_action_id(0x06),
        Some(ImeAction::Done)
    );
    assert_eq!(ImeAction::from_android_action_id(1), None);
}

#[test]
fn omitted_observation_fields_deserialize_as_unknown() {
    let element: UiElement = serde_json::from_str(
            r#"{"className":"android.widget.Button","text":null,"resourceId":null,"contentDescription":null,"bounds":{"left":0,"top":0,"right":1,"bottom":1},"clickable":true,"enabled":true}"#,
        )
        .unwrap();
    assert_eq!(element.focused, None);
    assert_eq!(element.checked, None);
    assert_eq!(element.selected, None);
    let json = serde_json::to_value(&element).unwrap();
    assert!(json.get("focused").is_none());
    assert!(json.get("checked").is_none());
}

#[tokio::test]
async fn chrome_boot_alias_routes_to_driver_with_an_available_page() {
    let driver: Arc<dyn Robot> = Arc::new(StubRobot {
        device: Some(RobotDevice {
            id: "chrome:page".into(),
            name: "Page".into(),
            platform: Platform::Chrome,
            state: DeviceState::Available,
        }),
        list_error: false,
    });
    let registry = RobotRegistry::new(vec![Arc::clone(&driver)]);
    assert_eq!(
        registry.stream_context("chrome:browser").await.unwrap(),
        serde_json::json!({ "deviceId": "chrome:browser" })
    );
}

#[tokio::test]
async fn registry_forwards_context_editing_to_the_selected_driver() {
    let registry = RobotRegistry::new(vec![Arc::new(StubRobot {
        device: Some(RobotDevice {
            id: "ios-fixture".into(),
            name: "Fixture".into(),
            platform: Platform::Ios,
            state: DeviceState::Available,
        }),
        list_error: false,
    }) as Arc<dyn Robot>]);
    let context = WebContext {
        app_id: Some("fixture.app".into()),
        ..Default::default()
    };
    for result in [
        registry
            .type_text_in_context("ios-fixture", "text", Some(&context))
            .await,
        registry
            .clear_text_in_context("ios-fixture", Some(&context))
            .await,
    ] {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("forwarded:ios-fixture:fixture.app")
        );
    }
}

#[test]
fn context_queries_match_constraints_without_runtime_identity() {
    let actual = WebContext {
        app_id: Some("fixture.app".into()),
        page: Some(WebPageSelector {
            url: Some("https://fixture/".into()),
            title: Some("Fixture".into()),
        }),
        element_ref: Some("runtime:1".into()),
        frame_path: vec![WebFrameSelector {
            css: Some("iframe#payment".into()),
            name: Some("payment".into()),
            ..Default::default()
        }],
    };
    let query = WebContext {
        app_id: actual.app_id.clone(),
        page: Some(WebPageSelector {
            url: Some("https://fixture/".into()),
            ..Default::default()
        }),
        frame_path: vec![WebFrameSelector {
            name: Some("payment".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(web_context_matches(&actual, &query));
    let wrong = WebContext {
        app_id: Some("another.app".into()),
        ..query
    };
    assert!(!web_context_matches(&actual, &wrong));
}

#[tokio::test]
async fn desktop_window_targets_route_without_changing_their_identity() {
    for (host, platform, target) in [
        ("windows-host", Platform::Windows, "windows:123:456"),
        ("macos-host", Platform::Macos, "macos:123:456"),
    ] {
        let driver: Arc<dyn Robot> = Arc::new(StubRobot {
            device: Some(RobotDevice {
                id: host.into(),
                name: "Desktop".into(),
                platform,
                state: DeviceState::Available,
            }),
            list_error: false,
        });
        let registry = RobotRegistry::new(vec![Arc::clone(&driver)]);
        assert_eq!(
            registry.stream_context(target).await.unwrap(),
            serde_json::json!({ "deviceId": target })
        );
        assert!(matches!(
            registry.stream_context("windows-other").await,
            Err(RobotError::DeviceNotFound(_))
        ));
        let empty = RobotRegistry::new(vec![]);
        assert!(matches!(
            empty.stream_context(target).await,
            Err(RobotError::DeviceNotFound(_))
        ));
    }
}

#[tokio::test]
async fn routes_to_a_healthy_driver_when_another_driver_is_unavailable() {
    let registry = RobotRegistry::new(vec![
        Arc::new(StubRobot {
            device: None,
            list_error: true,
        }) as Arc<dyn Robot>,
        Arc::new(StubRobot {
            device: Some(RobotDevice {
                id: "ios-sim".into(),
                name: "iPhone".into(),
                platform: Platform::Ios,
                state: DeviceState::Available,
            }),
            list_error: false,
        }),
    ]);

    assert_eq!(registry.list_devices().await.unwrap().len(), 1);
    assert_eq!(registry.take_screenshot("ios-sim").await.unwrap(), vec![7]);
}

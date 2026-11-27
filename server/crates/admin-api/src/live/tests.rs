use super::{LiveInput, LiveQuery, android_input, android_live};
use crate::{ActivityLog, AdminState};
use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
};
use robot_core::{Bounds, DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use std::sync::{Arc, Mutex as StdMutex};

struct LiveRobot {
    actions: Arc<StdMutex<Vec<String>>>,
    flutter: bool,
}

#[async_trait]
impl Robot for LiveRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![
            RobotDevice {
                id: "phone".into(),
                name: "Phone".into(),
                platform: Platform::Android,
                state: DeviceState::Available,
            },
            RobotDevice {
                id: "offline".into(),
                name: "Offline".into(),
                platform: Platform::Android,
                state: DeviceState::Offline,
            },
            RobotDevice {
                id: "browser".into(),
                name: "Chrome".into(),
                platform: Platform::Chrome,
                state: DeviceState::Available,
            },
        ])
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        panic!("live inspection must not capture screenshots: {device_id}")
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("tap {device_id} {},{}", point.x, point.y));
        Ok(())
    }

    async fn launch_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("unused".into()))
    }

    async fn open_url(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("unused".into()))
    }

    async fn clear_text(&self, device_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("clear {device_id}"));
        Ok(())
    }
    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("type {device_id} {text}"));
        Ok(())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "swipe {device_id} {},{} {},{} {}",
            start.x,
            start.y,
            end.x,
            end.y,
            duration_ms.unwrap_or(0)
        ));
        Ok(())
    }

    async fn install_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("unused".into()))
    }

    async fn uninstall_app(&self, _: &str, _: &str) -> Result<(), RobotError> {
        Err(RobotError::Unsupported("unused".into()))
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        if device_id != "phone" {
            return Err(RobotError::DeviceNotFound(device_id.to_owned()));
        }
        Ok(vec![sample_element("Save")])
    }

    async fn list_flutter_elements(&self, _device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        if !self.flutter {
            return Ok(Vec::new());
        }
        Ok(vec![sample_element("Flutter Save")])
    }
}

fn sample_element(text: &str) -> UiElement {
    UiElement {
        class_name: "Button".into(),
        resource_id: Some("save-button".into()),
        text: Some(text.into()),
        content_description: None,
        bounds: Bounds {
            left: 10,
            top: 20,
            right: 110,
            bottom: 60,
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

fn state(flutter: bool) -> (AdminState, Arc<StdMutex<Vec<String>>>) {
    let actions = Arc::new(StdMutex::new(Vec::new()));
    let admin = AdminState::new(
        Arc::new(LiveRobot {
            actions: Arc::clone(&actions),
            flutter,
        }),
        ActivityLog::new(8),
        Some("test-token".into()),
    );
    (admin, actions)
}

fn authorized() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());
    headers
}

#[tokio::test]
async fn clear_text_uses_the_robot_boundary_without_typing_an_empty_string() {
    let (admin, actions) = state(false);
    let response = android_input(
        Path("phone".into()),
        State(admin),
        authorized(),
        Json(LiveInput {
            action: "clear".into(),
            x: None,
            y: None,
            end_x: None,
            end_y: None,
            text: None,
            duration_ms: None,
            mode: None,
            delta_x: None,
            delta_y: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(*actions.lock().unwrap(), vec!["clear phone"]);
}

#[tokio::test]
async fn live_inspection_emits_elements_without_capturing_screenshots() {
    let (admin, _) = state(false);
    let response = android_live(
        Path("phone".into()),
        Query(LiveQuery {
            max_frames: Some(1),
        }),
        State(admin),
        authorized(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("event: frame"), "{text}");
    assert!(text.contains("event: elements"), "{text}");
    assert!(text.contains("Save"), "{text}");
    assert!(text.contains("\"source\":\"native\""), "{text}");
}

#[tokio::test]
async fn live_stream_uses_flutter_elements_when_native_nodes_have_no_label() {
    let actions = Arc::new(StdMutex::new(Vec::new()));
    let admin = AdminState::new(
        Arc::new(UnlabeledNative {
            inner: LiveRobot {
                actions,
                flutter: true,
            },
        }),
        ActivityLog::new(8),
        Some("test-token".into()),
    );
    let response = android_live(
        Path("phone".into()),
        Query(LiveQuery {
            max_frames: Some(1),
        }),
        State(admin),
        authorized(),
    )
    .await;
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("Flutter Save"), "{text}");
    assert!(text.contains("\"source\":\"flutter\""), "{text}");
}

struct UnlabeledNative {
    inner: LiveRobot,
}

#[async_trait]
impl Robot for UnlabeledNative {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        self.inner.list_devices().await
    }
    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.inner.take_screenshot(device_id).await
    }
    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.inner.tap(device_id, point).await
    }
    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.inner.launch_app(device_id, app_id).await
    }
    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.inner.open_url(device_id, url, app_id).await
    }
    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.inner.type_text(device_id, text).await
    }
    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.inner.swipe(device_id, start, end, duration_ms).await
    }
    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
        self.inner.install_app(device_id, apk_path).await
    }
    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.inner.uninstall_app(device_id, app_id).await
    }
    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        let mut elements = self.inner.list_elements(device_id).await?;
        elements[0].text = None;
        Ok(elements)
    }
    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.inner.list_flutter_elements(device_id).await
    }
}

#[tokio::test]
async fn live_control_rejects_other_platforms_and_forwards_android_input() {
    let (admin, actions) = state(false);
    let headers = authorized();

    let rejected = android_live(
        Path("browser".into()),
        Query(LiveQuery::default()),
        State(admin.clone()),
        headers.clone(),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    let offline = android_input(
        Path("offline".into()),
        State(admin.clone()),
        headers.clone(),
        Json(LiveInput {
            action: "tap".into(),
            x: Some(1.0),
            y: Some(2.0),
            end_x: None,
            end_y: None,
            text: None,
            duration_ms: None,
            mode: None,
            delta_x: None,
            delta_y: None,
        }),
    )
    .await;
    assert_eq!(offline.status(), StatusCode::BAD_REQUEST);

    let tap = android_input(
        Path("phone".into()),
        State(admin.clone()),
        headers.clone(),
        Json(LiveInput {
            action: "tap".into(),
            x: Some(12.4),
            y: Some(40.0),
            end_x: None,
            end_y: None,
            text: None,
            duration_ms: None,
            mode: None,
            delta_x: None,
            delta_y: None,
        }),
    )
    .await;
    assert_eq!(tap.status(), StatusCode::NO_CONTENT);

    let typed = android_input(
        Path("phone".into()),
        State(admin.clone()),
        headers.clone(),
        Json(LiveInput {
            action: "type".into(),
            x: None,
            y: None,
            end_x: None,
            end_y: None,
            text: Some("  hello  ".into()),
            duration_ms: None,
            mode: None,
            delta_x: None,
            delta_y: None,
        }),
    )
    .await;
    assert_eq!(typed.status(), StatusCode::NO_CONTENT);

    let missing = android_input(
        Path("phone".into()),
        State(admin),
        headers,
        Json(LiveInput {
            action: "type".into(),
            x: None,
            y: None,
            end_x: None,
            end_y: None,
            text: Some("   ".into()),
            duration_ms: None,
            mode: None,
            delta_x: None,
            delta_y: None,
        }),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);

    let recorded = actions.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![
            "tap phone 12.4,40".to_owned(),
            "type phone hello".to_owned(),
        ]
    );
}

#[tokio::test]
async fn live_endpoints_require_the_admin_token() {
    let (admin, _) = state(false);
    let response = android_live(
        Path("phone".into()),
        Query(LiveQuery::default()),
        State(admin),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

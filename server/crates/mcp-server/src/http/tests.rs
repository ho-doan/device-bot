use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use robot_core::{Point, Robot, RobotDevice, RobotError, UiElement};
use tower::ServiceExt;

use super::*;

#[derive(Default)]
struct NoopRobot {
    preparations: std::sync::Mutex<Vec<Value>>,
}

#[async_trait]
impl Robot for NoopRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![RobotDevice {
            id: "sim".into(),
            name: "Simulator".into(),
            platform: robot_core::Platform::Ios,
            state: robot_core::DeviceState::Available,
        }])
    }
    async fn ui_runner_status(&self, _: &str) -> Result<robot_core::UiRunnerStatus, RobotError> {
        Ok(robot_core::UiRunnerStatus {
            ready: false,
            state: "disconnected".into(),
            detail: "No live helper".into(),
            cache_ready: true,
            expected_version: Some("v2".into()),
            ..Default::default()
        })
    }
    async fn prepare_ui_runner(
        &self,
        id: &str,
        app: Option<&str>,
        reinstall: bool,
    ) -> Result<robot_core::UiRunnerStatus, RobotError> {
        self.preparations
            .lock()
            .unwrap()
            .push(json!({"deviceId":id,"appId":app,"reinstall":reinstall}));
        Ok(robot_core::UiRunnerStatus {
            ready: true,
            state: "ready".into(),
            detail: "prepared".into(),
            ..Default::default()
        })
    }
    async fn take_screenshot(&self, _: &str) -> Result<Vec<u8>, RobotError> {
        Err(RobotError::Unsupported("unused".into()))
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

fn initialize_body() -> &'static str {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}"#
}

#[tokio::test]
async fn requires_bearer_auth_and_preserves_lifecycle_in_one_session() {
    let app = router(Arc::new(NoopRobot::default()), Some("secret".into()));
    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .body(Body::from(initialize_body()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let initialized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer secret")
                .body(Body::from(initialize_body()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initialized.status(), StatusCode::OK);
    let session = initialized.headers()[SESSION_HEADER]
        .to_str()
        .unwrap()
        .to_owned();

    let before_ready = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer secret")
                .header(SESSION_HEADER, session)
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(before_ready.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_session_id_never_replaces_bearer_authentication_on_later_requests() {
    let app = router(Arc::new(NoopRobot::default()), Some("secret".into()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer secret")
                .body(Body::from(initialize_body()))
                .unwrap(),
        )
        .await
        .unwrap();
    let session = response.headers()[SESSION_HEADER]
        .to_str()
        .unwrap()
        .to_owned();
    for method in ["POST", "DELETE"] {
        for authorization in [None, Some("Bearer revoked-or-wrong")] {
            let mut request = Request::builder()
                .method(method)
                .uri("/mcp")
                .header(SESSION_HEADER, &session);
            if let Some(token) = authorization {
                request = request.header(header::AUTHORIZATION, token);
            }
            let response = app
                .clone()
                .oneshot(
                    request
                        .body(Body::from(
                            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {authorization:?}"
            );
        }
    }
    // The rejected DELETE did not destroy the legitimate client's session.
    let valid = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer secret")
                .header(SESSION_HEADER, session)
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(valid.status(), StatusCode::OK);
}

#[tokio::test]
async fn returns_an_sse_message_when_the_client_requests_it() {
    let response = router(Arc::new(NoopRobot::default()), None)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::from(initialize_body()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
}
#[tokio::test]
async fn lan_preflight_allows_private_origins_and_exposes_the_session_header() {
    let response = router(Arc::new(NoopRobot::default()), Some("secret".into()))
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/mcp")
                .header(header::ORIGIN, "http://192.168.1.20:3337")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "authorization,mcp-session-id",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "http://192.168.1.20:3337"
    );
    let allowed = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
        .to_str()
        .unwrap();
    assert!(allowed.to_ascii_lowercase().contains("authorization"));
    assert!(allowed.contains("mcp-session-id"));
    assert!(
        response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
            .to_str()
            .unwrap()
            .contains("mcp-session-id")
    );
    let denied = router(Arc::new(NoopRobot::default()), Some("secret".into()))
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/mcp")
                .header(header::ORIGIN, "http://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[tokio::test]
async fn liveness_is_available_without_authentication_or_device_checks() {
    let response = router(Arc::new(NoopRobot::default()), Some("secret".into()))
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["service"], "device-bot");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn health_is_read_only_and_reboot_is_authenticated_and_device_scoped() {
    let robot = Arc::new(NoopRobot::default());
    let app = router(robot.clone(), Some("test-secret".into()));
    for (method, uri, body) in [
        ("GET", "/health?deviceId=sim", ""),
        ("POST", "/reboot", r#"{"deviceId":"sim"}"#),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert!(robot.preparations.lock().unwrap().is_empty());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health?deviceId=sim")
                .header(header::AUTHORIZATION, "Bearer test-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    let status: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status["server"]["healthy"], true);
    assert_eq!(status["devices"][0]["ready"], false);
    assert_eq!(status["devices"][0]["cacheReady"], true);
    assert!(robot.preparations.lock().unwrap().is_empty());
    for (body, expected) in [
        (r#"{"deviceId":"sim", "appId":"app"}"#, true),
        (r#"{"deviceId":"sim", "reinstall":false}"#, false),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/reboot")
                    .header(header::AUTHORIZATION, "Bearer test-secret")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            robot.preparations.lock().unwrap().last().unwrap()["reinstall"],
            expected
        );
    }
    for body in [
        r#"{"deviceId":""}"#,
        r#"{"deviceId":"sim","appId":""}"#,
        r#"{"deviceId":"sim", "wipe":true}"#,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/reboot")
                    .header(header::AUTHORIZATION, "Bearer test-secret")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(robot.preparations.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn lan_origin_serves_qc_web_and_nested_report_yaml() {
    use std::fs;

    let root = std::env::temp_dir().join(format!("device-bot-lan-{}", Uuid::new_v4()));
    let qc = root.join("qc-web");
    fs::create_dir_all(qc.join("assets")).unwrap();
    fs::write(
        qc.join("index.html"),
        r#"<!doctype html><base href="/"><script src="main.dart.js"></script>"#,
    )
    .unwrap();
    fs::write(qc.join("main.dart.js"), b"window.qc=true;").unwrap();
    fs::write(qc.join("assets/app.js"), b"asset").unwrap();
    fs::write(root.join("secret.txt"), b"leaked").unwrap();

    let store = robot_report::Store::open(root.join("history")).unwrap();
    let source = root.join("case");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("report.json"),
        serde_json::to_vec(&json!({
            "name": "lan",
            "status": "PASS",
            "selected_device": {"platform": "android", "id": "dev"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("scenario.yaml"),
        "url: https://shop.example.com/app\n",
    )
    .unwrap();
    let summary = store
        .ingest_flow(robot_report::FlowIngest {
            name: "LAN yaml".into(),
            source: "qc".into(),
            case_dir: source,
            app_id: None,
            url: None,
        })
        .unwrap();
    let run = store.run(&summary.project_id, &summary.id).unwrap();
    let yaml = run.cases[0]
        .files
        .iter()
        .find(|file| file.name.ends_with("scenario.yaml"))
        .unwrap();

    let app = lan_router(
        Arc::new(NoopRobot::default()),
        Some("secret".into()),
        Some(qc),
        Some(store),
    );

    for uri in ["/qc", "/qc/"] {
        let qc_home = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(qc_home.status(), StatusCode::OK, "{uri}");
        assert_eq!(
            qc_home.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let html = String::from_utf8(
            axum::body::to_bytes(qc_home.into_body(), 10_000)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("<base href=\"/qc/\">"), "{uri}");
        assert!(!html.contains("<base href=\"/\">"), "{uri}");
    }

    let js = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/qc/main.dart.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(js.status(), StatusCode::OK);
    assert_eq!(
        js.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );

    let asset = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/qc/assets/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);

    let escape = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/qc/../secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let escaped = axum::body::to_bytes(escape.into_body(), 10_000)
        .await
        .unwrap();
    assert_ne!(escaped.as_ref(), b"leaked");

    let reports_health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reports/health")
                .header("host", "192.168.1.20:3337")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reports_health.status(), StatusCode::OK);
    let health: Value = serde_json::from_slice(
        &axum::body::to_bytes(reports_health.into_body(), 1000)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(health["service"], "robot-report");

    let reports_home = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reports/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reports_home.status(), StatusCode::OK);
    let page = String::from_utf8(
        axum::body::to_bytes(reports_home.into_body(), 50_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(page.contains("assets/report.js"));

    let report_js = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reports/assets/report.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(report_js.status(), StatusCode::OK);
    let script = String::from_utf8(
        axum::body::to_bytes(report_js.into_body(), 50_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(script.contains("pathname.startsWith('/reports')"));

    let yaml_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/reports/api/projects/{}/runs/{}/cases/0/files/{}",
                    summary.project_id, summary.id, yaml.id
                ))
                .header("host", "192.168.1.20:3337")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(yaml_response.status(), StatusCode::OK);
    let yaml_body = axum::body::to_bytes(yaml_response.into_body(), 10_000)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&yaml_body).contains("https://shop.example.com/app"));

    let _ = fs::remove_dir_all(&root);
}

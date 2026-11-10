//! Read-only physical-device and live XCTest helper health probes.
use crate::{xctest_agent::XCTestAgentBridge, xctest_robot::IosXCTestRobot};
use robot_core::{
    DeviceState, Robot,
    health::{CheckStatus, HealthCheck, HealthReport},
};
use serde_json::Value;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

/// Checks a physical iPhone without launching/reinstalling apps or rebooting it.
pub async fn physical_health(xcrun: &str, device: &str) -> HealthReport {
    let project = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("xctest-runner/MobileMcpRealRunner.xcodeproj");
    physical_health_with_project(xcrun, device, &project).await
}
pub async fn physical_health_with_project(
    xcrun: &str,
    device: &str,
    project: &std::path::Path,
) -> HealthReport {
    let start = Instant::now();
    let robot = IosXCTestRobot::with_artifact_dir(xcrun, XCTestAgentBridge::new(None), "artifacts")
        .with_runner_project(project.to_path_buf());
    let mut checks = Vec::new();
    match timeout(Duration::from_secs(8), robot.list_devices()).await {
        Ok(Ok(devices)) => {
            checks.push(item(
                "coredevice",
                CheckStatus::Pass,
                "CoreDevice responded",
            ));
            let connected = devices
                .iter()
                .any(|d| d.id.eq_ignore_ascii_case(device) && d.state == DeviceState::Available);
            checks.push(item(
                "device_connection",
                if connected {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                if connected {
                    "Physical device is connected/available"
                } else {
                    "Device is absent, offline or not ready; check pairing/trust/connection"
                },
            ));
            if connected {
                let derived = project.parent().unwrap().join("build");
                let cache = crate::runner_cache::RunnerCache {
                    project,
                    derived_data: &derived,
                    simulator: false,
                };
                match cache.version() {
                    Ok(version) => checks.push(agent_check_for(false, device, &version).await),
                    Err(error) => {
                        checks.push(item("xctest_assets", CheckStatus::Fail, error.to_string()))
                    }
                }
            }
        }
        Ok(Err(e)) => checks.push(item("coredevice", CheckStatus::Fail, e.to_string())),
        Err(_) => checks.push(item(
            "coredevice",
            CheckStatus::Fail,
            "CoreDevice did not respond within 8 seconds",
        )),
    }
    if let Ok(report) = robot.connection_report(device, true).await {
        for probe in report.checks {
            let name = match probe.name {
                "USB" => "usb_connection",
                "Xcode destination" | "Xcode" => "xcode_destination",
                _ => continue,
            };
            let status = match probe.status {
                "pass" => CheckStatus::Pass,
                "fail" => CheckStatus::Fail,
                "warn" => CheckStatus::Warn,
                _ => CheckStatus::Unknown,
            };
            checks.push(item(name, status, probe.detail));
        }
    }
    HealthReport::from_checks(
        "ios_physical",
        device,
        start.elapsed().as_millis(),
        checks,
        vec![],
    )
}

fn item(name: &str, status: CheckStatus, detail: impl Into<String>) -> HealthCheck {
    HealthCheck {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

/// Uses the existing bridge's authenticated /health endpoint. No helper is
/// installed or started by this check. A missing/old bridge is not healthy.
pub async fn agent_check(simulator: bool) -> HealthCheck {
    agent_check_target(simulator, None).await
}
pub async fn agent_check_for(simulator: bool, device: &str, expected_version: &str) -> HealthCheck {
    agent_check_target(simulator, Some((device, expected_version))).await
}
async fn agent_check_target(simulator: bool, target: Option<(&str, &str)>) -> HealthCheck {
    let (address_key, token_key, fallback) = if simulator {
        (
            "DEVICE_BOT_IOS_SIM_AGENT_BIND",
            "DEVICE_BOT_IOS_SIM_AGENT_TOKEN",
            "127.0.0.1:18788",
        )
    } else {
        (
            "DEVICE_BOT_IOS_AGENT_BIND",
            "DEVICE_BOT_IOS_AGENT_TOKEN",
            "127.0.0.1:18787",
        )
    };
    let address = std::env::var(address_key).unwrap_or_else(|_| fallback.into());
    let address: SocketAddr = match address.parse() {
        Ok(a) => a,
        Err(_) => {
            return item(
                "agent_heartbeat",
                CheckStatus::Unknown,
                "Invalid agent bridge bind address",
            );
        }
    };
    let address = SocketAddr::new(
        if address.ip().is_unspecified() {
            if address.is_ipv4() {
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            } else {
                std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
            }
        } else {
            address.ip()
        },
        address.port(),
    );
    let token = std::env::var(token_key).ok();
    match timeout(
        Duration::from_secs(5),
        read_agent_health_target(address, token.as_deref(), target),
    )
    .await
    {
        Ok(Ok(())) => item(
            "agent_heartbeat",
            CheckStatus::Pass,
            if target.is_some() {
                "Live helper confirmed device identity, version and selected target app"
            } else {
                "Live helper heartbeat confirmed (identity/version not checked)"
            },
        ),
        Ok(Err(e)) => item(e.name, e.status, e.detail),
        Err(_) => item(
            "agent_heartbeat",
            CheckStatus::Fail,
            "MCP bridge/helper did not respond within 5 seconds",
        ),
    }
}

#[derive(Debug)]
struct AgentHealthFailure {
    name: &'static str,
    status: CheckStatus,
    detail: &'static str,
}
fn failure(name: &'static str, status: CheckStatus, detail: &'static str) -> AgentHealthFailure {
    AgentHealthFailure {
        name,
        status,
        detail,
    }
}

#[cfg(test)]
async fn read_agent_health(
    address: SocketAddr,
    token: Option<&str>,
) -> Result<(), AgentHealthFailure> {
    read_agent_health_target(address, token, None).await
}
async fn read_agent_health_target(
    address: SocketAddr,
    token: Option<&str>,
    target: Option<(&str, &str)>,
) -> Result<(), AgentHealthFailure> {
    let path = if let Some((device, version)) = target {
        format!(
            "/health?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("deviceId", device)
                .append_pair("expectedVersion", version)
                .finish()
        )
    } else {
        "/health".into()
    };
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(token) = token {
        axum::http::HeaderValue::from_str(token).map_err(|_| {
            failure(
                "agent_authentication",
                CheckStatus::Unknown,
                "Invalid agent token header",
            )
        })?;
        request.push_str("x-mobilemcp-token: ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    let mut connection = TcpStream::connect(address).await.map_err(|_| {
        failure(
            "agent_bridge",
            CheckStatus::Unknown,
            "MCP agent bridge is not listening; start the MCP session, then recheck",
        )
    })?;
    connection
        .write_all(request.as_bytes())
        .await
        .map_err(|_| {
            failure(
                "agent_bridge",
                CheckStatus::Fail,
                "Agent health request failed",
            )
        })?;
    let mut bytes = Vec::new();
    connection
        .take(65537)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| {
            failure(
                "agent_bridge",
                CheckStatus::Fail,
                "Agent health response failed",
            )
        })?;
    if bytes.len() > 65536 {
        return Err(failure(
            "agent_health_protocol",
            CheckStatus::Fail,
            "Agent health response exceeds limit",
        ));
    }
    decode_health_response(&bytes)?;
    if let Some((device, version)) = target {
        let raw = std::str::from_utf8(&bytes).unwrap();
        let body: Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
        if body["helper"]["deviceId"].as_str() != Some(device)
            || body["helper"]["agentVersion"].as_str() != Some(version)
        {
            return Err(failure(
                "agent_version",
                CheckStatus::Fail,
                "XCTest helper identity/version is missing or outdated; prepare this device's helper",
            ));
        }
        if body["helper"]["appForeground"] != true {
            return Err(failure(
                "agent_target",
                CheckStatus::Warn,
                "Select and launch the target app; helper is not bound to a foreground app",
            ));
        }
        if body["helper"]["bundleId"]
            .as_str()
            .is_none_or(|v| v.is_empty())
        {
            return Err(failure(
                "agent_target",
                CheckStatus::Warn,
                "Helper is current but no target app is selected; launch the app through Device Bot",
            ));
        }
    }
    Ok(())
}

fn decode_health_response(bytes: &[u8]) -> Result<(), AgentHealthFailure> {
    let invalid = || {
        failure(
            "agent_health_protocol",
            CheckStatus::Fail,
            "Invalid agent health response",
        )
    };
    let response = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let (headers, body) = response.split_once("\r\n\r\n").ok_or_else(invalid)?;
    let status = headers
        .lines()
        .next()
        .and_then(|s| s.split_whitespace().nth(1));
    match status {
        Some("200") => {}
        Some("401") => {
            return Err(failure(
                "agent_authentication",
                CheckStatus::Fail,
                "Agent health authentication failed; verify the configured token",
            ));
        }
        Some("404") => {
            return Err(failure(
                "mcp_health_endpoint",
                CheckStatus::Unknown,
                "Running MCP bridge is older than this health client; restart the session with the updated MCP build",
            ));
        }
        Some("503") => {
            return Err(failure(
                "agent_heartbeat",
                CheckStatus::Fail,
                "MCP bridge responded but its XCTest helper did not",
            ));
        }
        _ => return Err(invalid()),
    }
    let value: Value = serde_json::from_str(body).map_err(|_| invalid())?;
    if value.get("healthy").and_then(Value::as_bool) != Some(true) {
        return Err(failure(
            "agent_heartbeat",
            CheckStatus::Fail,
            "Agent health was not confirmed",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn http_client_checks_the_real_bridge_and_helper_result() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let bridge = XCTestAgentBridge::new(Some("test-token".into()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = bridge.router();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let error = read_agent_health(address, None).await.unwrap_err();
        assert_eq!(error.name, "agent_authentication");
        let helper = tokio::spawn(async move {
            let command = timeout(Duration::from_secs(1), async {
                loop {
                    let response = bridge
                        .router()
                        .oneshot(
                            Request::builder()
                                .uri("/next")
                                .header("x-mobilemcp-token", "test-token")
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    if response.status() == axum::http::StatusCode::OK {
                        let bytes = axum::body::to_bytes(response.into_body(), 4096)
                            .await
                            .unwrap();
                        break serde_json::from_slice::<Value>(&bytes).unwrap();
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(command["method"], "health");
            let response = bridge
                .router()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/result")
                        .header("x-mobilemcp-token", "test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"id":command["id"], "ok":true}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        });
        let result = timeout(
            Duration::from_secs(3),
            read_agent_health(address, Some("test-token")),
        )
        .await;
        helper.await.unwrap();
        server.abort();
        result.unwrap().unwrap();
    }

    #[test]
    fn heartbeat_requires_explicit_success_not_just_http() {
        assert!(decode_health_response(b"HTTP/1.1 200 OK\r\n\r\n{\"healthy\":true}").is_ok());
        for response in [
            b"HTTP/1.1 200 OK\r\n\r\n{}".as_slice(),
            b"HTTP/1.1 401 Unauthorized\r\n\r\n{}",
            b"HTTP/1.1 503 Unavailable\r\n\r\n{}",
        ] {
            assert!(decode_health_response(response).is_err());
        }
    }
}

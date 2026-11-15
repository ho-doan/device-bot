//! Own bridge startup, driver assembly, transport serving, and shutdown.

use crate::{
    assets,
    companion::launch_flutter_companion,
    config::{self, ServerConfig},
    flavor, qc_connection,
};
use device_bot::Server;
use robot_android::AndroidRobot;
use robot_chrome::ChromeRobot;
use robot_core::RobotRegistry;
use robot_ios_real::{IosXCTestRobot, XCTestAgentBridge, XCTestAgentLauncher};
use robot_ios_sim::IosSimulatorRobot;
use robot_macos::MacosRobot;
use robot_windows::WindowsRobot;
use std::{net::SocketAddr, sync::Arc};

pub(super) async fn serve(listen_address: Option<SocketAddr>) -> std::io::Result<()> {
    let ServerConfig {
        adb_path,
        xcrun_path,
        artifact_dir,
        ios_agent_address,
        ios_agent_token,
        ios_agent_hosts,
        ios_sim_agent_address,
        ios_sim_agent_token,
    } = ServerConfig::from_env()?;
    let ios_agent = XCTestAgentBridge::new(ios_agent_token.clone());
    let listener = tokio::net::TcpListener::bind(ios_agent_address).await?;
    let ios_agent_address = listener.local_addr()?;
    let agent_router = ios_agent.router();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, agent_router).await {
            eprintln!("iOS XCTest agent bridge stopped: {error}");
        }
    });
    let ios_agent_launcher = ios_agent_hosts
        .map(|hosts| {
            let project = assets::xctest_project("ios-real");
            let derived_data = project
                .parent()
                .expect("real-iOS XCTest project has a parent directory")
                .join("build");
            XCTestAgentLauncher::new(
                "xcodebuild",
                project,
                derived_data,
                hosts,
                ios_agent_address.port(),
                ios_agent_token,
            )
        })
        .transpose()
        .map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
        })?;
    let ios_sim_agent = XCTestAgentBridge::new(ios_sim_agent_token);
    let ios_sim_listener = tokio::net::TcpListener::bind(ios_sim_agent_address).await?;
    let ios_sim_agent_address = ios_sim_listener.local_addr()?;
    let ios_sim_agent_router = ios_sim_agent.router();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(ios_sim_listener, ios_sim_agent_router).await {
            eprintln!("iOS Simulator XCTest agent bridge stopped: {error}");
        }
    });
    let mut real_ios =
        IosXCTestRobot::with_artifact_dir(xcrun_path.clone(), ios_agent, artifact_dir.clone())
            .with_runner_project(assets::xctest_project("ios-real"));
    if let Some(launcher) = ios_agent_launcher {
        real_ios = real_ios.with_agent_launcher(launcher);
    }
    let drivers: Vec<Arc<dyn robot_core::Robot>> = vec![
        Arc::new(AndroidRobot::new(adb_path, artifact_dir.clone())),
        Arc::new(ChromeRobot::from_env()),
        Arc::new(real_ios),
        Arc::new(IosSimulatorRobot::with_xctest_runner_and_agent(
            xcrun_path,
            "xcodebuild",
            assets::xctest_project("ios-sim"),
            artifact_dir,
            ios_sim_agent,
            ios_sim_agent_address.ip().to_string(),
            ios_sim_agent_address.port(),
        )),
        Arc::new(MacosRobot::from_env()),
        Arc::new(WindowsRobot::from_env()),
    ];
    let robot = Arc::new(RobotRegistry::new(drivers));
    match listen_address {
        None => device_bot::stdio::serve(Server::new(robot)).await,
        Some(address) => {
            let listener = tokio::net::TcpListener::bind(address).await?;
            let listen_address = listener.local_addr()?;
            let bearer_token = config::resolve_bearer_token();
            if let Err(error) = qc_connection::publish(listen_address, &bearer_token) {
                eprintln!("QC local connection discovery unavailable: {error}");
            }
            if flavor::FLAVOR == flavor::Flavor::Dev
                && listen_address.port() == flavor::Flavor::Prod.mcp_port()
            {
                eprintln!(
                    "dev is listening on the prod MCP port {}; prod uses 3337 and dev uses 3338",
                    listen_address.port()
                );
            }
            eprintln!(
                "Device Bot {} listening on http://{listen_address}",
                flavor::FLAVOR.name()
            );
            let mut flutter_companion = launch_flutter_companion(listen_address, &bearer_token);
            let reports = robot_report::Store::open(
                robot_report::default_reports_dir().unwrap_or(flavor::reports_dir()?),
            )
            .ok();
            if reports.is_none() {
                eprintln!("robot-report history is unavailable; /reports will not be served");
            }
            let qc_root = device_bot::qc_web_root();
            if qc_root.is_none() {
                eprintln!(
                    "QC web assets were not found; build server/flutter_app with `flutter build web --base-href /qc/` or set DEVICE_BOT_QC_WEB_DIR"
                );
            }
            let lan_hosts = config::discovered_lan_hosts().unwrap_or_default();
            for url in device_bot::advertise(listen_address, &lan_hosts) {
                eprintln!("QC: {url}/qc/");
                eprintln!("Reports: {url}/reports/");
            }
            let result = axum::serve(
                listener,
                device_bot::http::lan_router(robot, Some(bearer_token), qc_root, reports)
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal())
            .await;
            if let Some(mut child) = flutter_companion.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            result
        }
    }
}

async fn shutdown_signal() {
    // Embedded flow runs install Tokio's process-wide signal handlers. Keep a
    // server listener alive too, otherwise SIGTERM is swallowed after a run.
    #[cfg(unix)]
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! {
            _ = terminate.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}

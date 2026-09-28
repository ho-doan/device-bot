use std::{net::SocketAddr, sync::Arc};

use mobile_mcp_server::Server;
use mobile_robot_android::AndroidRobot;
use mobile_robot_chrome::ChromeRobot;
use mobile_robot_core::RobotRegistry;
use mobile_robot_ios_real::{IosXCTestRobot, XCTestAgentBridge};
use mobile_robot_ios_sim::IosSimulatorRobot;
use mobile_robot_macos::MacosRobot;
use mobile_robot_windows::WindowsRobot;

fn android_adb_path() -> String {
    if let Ok(path) = std::env::var("ANDROID_ADB_PATH") {
        return path;
    }
    let executable = if cfg!(windows) { "adb.exe" } else { "adb" };
    for variable in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(path) = std::env::var_os(variable)
            .map(std::path::PathBuf::from)
            .map(|path| path.join("platform-tools").join(executable))
            .filter(|path| path.is_file())
        {
            return path.to_string_lossy().into_owned();
        }
    }
    executable.into()
}

async fn install_chrome_recording_tools() -> std::io::Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "--install-chrome-recording-tools is available only on macOS",
        ));
    }
    let brew = std::env::var("HOMEBREW_EXECUTABLE").unwrap_or_else(|_| "brew".into());
    let status = tokio::process::Command::new(&brew)
        .args(["install", "ffmpeg"])
        .status()
        .await
        .map_err(|error| {
            std::io::Error::other(format!("could not run {brew} install ffmpeg: {error}"))
        })?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "{brew} install ffmpeg exited with {}",
            status
                .code()
                .map_or_else(|| "unknown".into(), |code| code.to_string())
        )));
    }
    println!("Chrome recording tools installed. Restart mobile-mcp-server to use ffmpeg.");
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    if matches!(
        command.as_deref(),
        Some("--request-screen-recording-permission")
    ) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--request-screen-recording-permission accepts no arguments",
            ));
        }
        if mobile_robot_macos::request_screen_recording_permission() {
            println!("Screen Recording permission is already granted.");
            return Ok(());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "macOS displayed the Screen Recording permission request; approve it, then run this command again",
        ));
    }
    if matches!(command.as_deref(), Some("--install-chrome-recording-tools")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--install-chrome-recording-tools accepts no arguments",
            ));
        }
        return install_chrome_recording_tools().await;
    }
    if matches!(command.as_deref(), Some("--ios-development-team")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--ios-development-team accepts no arguments",
            ));
        }
        let identity_hint = std::env::var("MOBILE_MCP_IOS_SIGNING_IDENTITY").ok();
        let team = mobile_robot_ios_real::apple_development_team(identity_hint.as_deref())
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        println!("{team}");
        return Ok(());
    }
    if matches!(command.as_deref(), Some("--provision-ios-xctest")) {
        let device_id = arguments.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--provision-ios-xctest requires a paired iPhone UDID",
            )
        })?;
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--provision-ios-xctest accepts exactly one device UDID",
            ));
        }
        let project = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../robot-ios-real/xctest-runner/MobileMcpRealRunner.xcodeproj");
        let team = std::env::var("MOBILE_MCP_IOS_TEAM").ok();
        let identity_hint = std::env::var("MOBILE_MCP_IOS_SIGNING_IDENTITY").ok();
        mobile_robot_ios_real::provision_xctest_runner(
            &project,
            &device_id,
            team.as_deref(),
            identity_hint.as_deref(),
        )
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        println!("iOS XCTest runner provisioned for {device_id}");
        return Ok(());
    }
    let adb_path = android_adb_path();
    let xcrun_path = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let artifact_dir =
        std::env::var("MOBILE_MCP_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into());
    let ios_agent_address =
        std::env::var("MOBILE_MCP_IOS_AGENT_BIND").unwrap_or_else(|_| "127.0.0.1:18787".into());
    let ios_agent_address: SocketAddr = ios_agent_address.parse().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid MOBILE_MCP_IOS_AGENT_BIND address: {error}"),
        )
    })?;
    let ios_agent = XCTestAgentBridge::new(std::env::var("MOBILE_MCP_IOS_AGENT_TOKEN").ok());
    let listener = tokio::net::TcpListener::bind(ios_agent_address).await?;
    let agent_router = ios_agent.router();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, agent_router).await {
            eprintln!("iOS XCTest agent bridge stopped: {error}");
        }
    });
    let ios_sim_agent_address =
        std::env::var("MOBILE_MCP_IOS_SIM_AGENT_BIND").unwrap_or_else(|_| "127.0.0.1:18788".into());
    let ios_sim_agent_address: SocketAddr = ios_sim_agent_address.parse().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid MOBILE_MCP_IOS_SIM_AGENT_BIND address: {error}"),
        )
    })?;
    let ios_sim_agent =
        XCTestAgentBridge::new(std::env::var("MOBILE_MCP_IOS_SIM_AGENT_TOKEN").ok());
    let ios_sim_listener = tokio::net::TcpListener::bind(ios_sim_agent_address).await?;
    let ios_sim_agent_router = ios_sim_agent.router();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(ios_sim_listener, ios_sim_agent_router).await {
            eprintln!("iOS Simulator XCTest agent bridge stopped: {error}");
        }
    });
    let drivers: Vec<Arc<dyn mobile_robot_core::Robot>> = vec![
        Arc::new(AndroidRobot::new(adb_path, artifact_dir.clone())),
        Arc::new(ChromeRobot::from_env()),
        Arc::new(IosXCTestRobot::new(xcrun_path.clone(), ios_agent)),
        Arc::new(IosSimulatorRobot::with_xctest_runner_and_agent(
            xcrun_path,
            "xcodebuild",
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../robot-ios-sim/xctest-runner/MobileMcpSimulatorRunner.xcodeproj"),
            artifact_dir,
            ios_sim_agent,
            ios_sim_agent_address.ip().to_string(),
            ios_sim_agent_address.port(),
        )),
        Arc::new(MacosRobot::from_env()),
        Arc::new(WindowsRobot::from_env()),
    ];
    let robot = Arc::new(RobotRegistry::new(drivers));
    match command.as_deref() {
        None => mobile_mcp_server::stdio::serve(Server::new(robot)).await,
        Some("--listen") => {
            let address = arguments.next().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "--listen requires HOST:PORT",
                )
            })?;
            if arguments.next().is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "unexpected command-line argument",
                ));
            }
            let address: SocketAddr = address.parse().map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid listen address: {error}"),
                )
            })?;
            let bearer_token = std::env::var("MOBILEMCP_AUTH")
                .ok()
                .filter(|token| !token.is_empty());
            mobile_mcp_server::http::serve(address, robot, bearer_token).await
        }
        Some(argument) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown argument: {argument}"),
        )),
    }
}

use std::{net::SocketAddr, sync::Arc};

use mobile_mcp_server::Server;
use mobile_robot_android::AndroidRobot;
use mobile_robot_core::RobotRegistry;
use mobile_robot_ios_real::IosRealRobot;
use mobile_robot_ios_sim::IosSimulatorRobot;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let adb_path = std::env::var("ANDROID_ADB_PATH").unwrap_or_else(|_| "adb".into());
    let xcrun_path = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let go_ios_path = std::env::var("GO_IOS_PATH").unwrap_or_else(|_| "ios".into());
    let artifact_dir =
        std::env::var("MOBILE_MCP_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into());
    let drivers: Vec<Arc<dyn mobile_robot_core::Robot>> = vec![
        Arc::new(AndroidRobot::new(adb_path)),
        Arc::new(IosRealRobot::new(go_ios_path)),
        Arc::new(IosSimulatorRobot::new(xcrun_path, artifact_dir)),
    ];
    let robot = Arc::new(RobotRegistry::new(drivers));
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
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

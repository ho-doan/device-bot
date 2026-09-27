use std::{net::SocketAddr, sync::Arc};

use mobile_mcp_server::Server;
use mobile_robot_android::AndroidRobot;
use mobile_robot_core::RobotRegistry;
use mobile_robot_ios_real::IosRealRobot;
use mobile_robot_ios_sim::IosSimulatorRobot;

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

fn go_ios_path() -> String {
    if let Ok(path) = std::env::var("GO_IOS_PATH") {
        return path;
    }

    let local_tool = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tools/go/bin/go-ios");
    if local_tool.is_file() {
        return local_tool.to_string_lossy().into_owned();
    }

    "ios".into()
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let adb_path = android_adb_path();
    let xcrun_path = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let go_ios_path = go_ios_path();
    let artifact_dir =
        std::env::var("MOBILE_MCP_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into());
    let drivers: Vec<Arc<dyn mobile_robot_core::Robot>> = vec![
        Arc::new(AndroidRobot::new(adb_path, artifact_dir.clone())),
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

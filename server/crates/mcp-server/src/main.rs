use std::sync::Arc;

use mobile_mcp_server::Server;
use mobile_robot_android::AndroidRobot;
use mobile_robot_core::RobotRegistry;
use mobile_robot_ios_sim::IosSimulatorRobot;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let adb_path = std::env::var("ANDROID_ADB_PATH").unwrap_or_else(|_| "adb".into());
    let xcrun_path = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let artifact_dir =
        std::env::var("MOBILE_MCP_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into());
    let drivers: Vec<Arc<dyn mobile_robot_core::Robot>> = vec![
        Arc::new(AndroidRobot::new(adb_path)),
        Arc::new(IosSimulatorRobot::new(xcrun_path, artifact_dir)),
    ];
    mobile_mcp_server::stdio::serve(Server::new(Arc::new(RobotRegistry::new(drivers)))).await
}

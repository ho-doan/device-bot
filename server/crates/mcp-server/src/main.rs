use std::sync::Arc;

use mobile_mcp_server::Server;
use mobile_robot_android::AndroidRobot;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let adb_path = std::env::var("ANDROID_ADB_PATH").unwrap_or_else(|_| "adb".into());
    mobile_mcp_server::stdio::serve(Server::new(Arc::new(AndroidRobot::new(adb_path)))).await
}

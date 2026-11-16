//! Run on Windows with DEVICE_BOT_WINDOWS_ACCESSIBILITY=uia or msaa.
use robot_core::Robot;
use robot_windows::WindowsRobot;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = std::env::args().nth(1).ok_or(
        "usage: cargo run -p robot-windows --example accessibility_snapshot -- windows:<pid>:<hwnd>",
    )?;
    let robot = WindowsRobot::new("artifacts");
    // Native only: never queries the embedded Flutter bridge.
    let elements = robot.list_elements(&device).await?;
    println!("{}", serde_json::to_string_pretty(&elements)?);
    Ok(())
}

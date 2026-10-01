use std::{env, io, sync::Arc};

use mobile_robot_chrome::ChromeRobot;
use mobile_robot_core::RobotRegistry;
use mobile_robot_flow::{FlowDocument, FlowRunner};
use mobile_robot_macos::MacosRobot;

fn usage() -> ! {
    eprintln!("Usage: robot-flow [--dry-run] <flow.yaml> [<flow.yaml> ...]");
    eprintln!("       robot-flow --help");
    std::process::exit(2);
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut dry_run = false;
    let mut paths = Vec::new();
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--dry-run" => dry_run = true,
            "--help" | "-h" => usage(),
            value if value.starts_with('-') => usage(),
            value => paths.push(value.to_owned()),
        }
    }
    if paths.is_empty() {
        usage();
    }

    let runner = if dry_run {
        FlowRunner::dry_run()
    } else {
        let robot = RobotRegistry::new([
            Arc::new(ChromeRobot::from_env()) as Arc<dyn mobile_robot_core::Robot>,
            Arc::new(MacosRobot::from_env()),
        ]);
        FlowRunner::with_robot(Arc::new(robot))
    };

    for path in paths {
        let flow = FlowDocument::from_path(&path).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{path}: {error}"))
        })?;
        let report = runner
            .run(&flow)
            .await
            .map_err(|error| io::Error::other(format!("{path}: {error}")))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("encode report: {error}"),
                )
            })?
        );
    }
    Ok(())
}

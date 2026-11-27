//! Execute parsed CLI commands without coupling their handlers to server startup.

use crate::{assets, cli::Command, flavor, launcher, qc, update};
use robot_ios_sim::IosSimulatorRobot;

pub(super) async fn execute(command: Command) -> std::io::Result<()> {
    match command {
        Command::Launch { open_qc } => launcher::start(open_qc),
        Command::Qc => qc::open(),
        Command::Run(arguments) => launcher::run(arguments).await,
        Command::InitProject(path) => {
            let manifest = robot_project::init(std::path::Path::new(&path))?;
            println!(
                "Created {}\nRun: device-bot run {}",
                manifest.display(),
                path
            );
            Ok(())
        }
        Command::ProjectReport(path) => {
            robot_project::ensure_report(std::path::Path::new(&path)).map(|_| ())
        }
        Command::ProjectFixture { start, app, state } => {
            if start {
                robot_project::windows_fixture::start(
                    std::path::Path::new(&app),
                    std::path::Path::new(&state),
                )
            } else {
                robot_project::windows_fixture::stop(
                    std::path::Path::new(&app),
                    std::path::Path::new(&state),
                )
            }
        }
        Command::Help => {
            println!("{}", flavor::help());
            Ok(())
        }
        Command::McpHelp => {
            println!("device-bot mcp [--listen HOST:PORT]\nDefaults to JSON-RPC over stdio.");
            Ok(())
        }
        Command::Version => {
            if flavor::FLAVOR == flavor::Flavor::Dev {
                println!("{} dev", env!("CARGO_PKG_VERSION"));
            } else {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
            Ok(())
        }
        Command::CheckInstall => assets::check_install(),
        Command::Health(id) => health(id).await,
        Command::CheckUpdate => check_update(),
        Command::Upgrade => upgrade(),
        Command::RequestScreenRecordingPermission => request_screen_recording_permission(),
        Command::InstallChromeRecordingTools => install_chrome_recording_tools().await,
        Command::IosDevelopmentTeam => development_team().await,
        Command::ConfigureIosSigning => configure_signing().await,
        Command::ProvisionIosXctest(id) => provision_xctest(id).await,
        Command::Serve(address) => crate::bootstrap::serve(address).await,
    }
}

async fn health(device_id: String) -> std::io::Result<()> {
    let xcrun = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let simulator = IosSimulatorRobot::with_xctest_runner(
        &xcrun,
        "xcodebuild",
        assets::xctest_project("ios-sim"),
        "artifacts",
    );
    let sim_devices = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        robot_core::Robot::list_devices(&simulator),
    )
    .await;
    let is_sim = matches!(sim_devices, Ok(Ok(ref devices)) if
            devices.iter().any(|d| d.id.eq_ignore_ascii_case(&device_id)));
    let report = if is_sim {
        simulator
            .check_health(&device_id)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
    } else {
        robot_ios_real::health::physical_health_with_project(
            &xcrun,
            &device_id,
            &assets::xctest_project("ios-real"),
        )
        .await
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(std::io::Error::other)?
    );
    if !report.ready_for_new_session {
        return Err(std::io::Error::other(format!(
            "health check: {}",
            report.status
        )));
    }
    Ok(())
}

fn check_update() -> std::io::Result<()> {
    match update::check_latest_release(env!("CARGO_PKG_VERSION"))? {
        Some(update) => {
            let encoded = serde_json::to_string_pretty(&update).map_err(|error| {
                std::io::Error::other(format!("encode update information: {error}"))
            })?;
            println!("{encoded}");
        }
        None => println!("device-bot {} is up to date.", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

fn upgrade() -> std::io::Result<()> {
    if flavor::FLAVOR == flavor::Flavor::Dev {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the dev flavor does not replace the installed device-bot; run the prod binary with --upgrade",
        ));
    }
    match update::check_latest_release(env!("CARGO_PKG_VERSION"))? {
        Some(update) => {
            println!(
                "Upgrading device-bot {} -> {}...",
                update.current_version, update.latest_version
            );
            update::perform_upgrade(&update)?;
        }
        None => println!("device-bot {} is up to date.", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

fn request_screen_recording_permission() -> std::io::Result<()> {
    if robot_macos::request_screen_recording_permission() {
        println!("Screen Recording permission is already granted.");
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "macOS displayed the Screen Recording permission request; approve it, then run this command again",
    ))
}

async fn development_team() -> std::io::Result<()> {
    let identity_hint = std::env::var("DEVICE_BOT_IOS_SIGNING_IDENTITY").ok();
    let team = robot_ios_real::apple_development_team(identity_hint.as_deref())
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    println!("{team}");
    Ok(())
}

async fn configure_signing() -> std::io::Result<()> {
    let team = std::env::var("DEVICE_BOT_IOS_TEAM").ok();
    let identity = std::env::var("DEVICE_BOT_IOS_SIGNING_IDENTITY").ok();
    let project = assets::xctest_project("ios-real");
    let selected = robot_ios_real::configure_xctest_signing(&project, team.as_deref(), identity.as_deref())
            .await.map_err(|error| std::io::Error::other(error.to_string()))?
            .ok_or_else(|| std::io::Error::other("Signing script is missing from XCTest assets; reinstall the complete Device Bot bundle"))?;
    println!(
        "{}",
        serde_json::json!({"configured": true, "teamId": selected, "project": project})
    );
    Ok(())
}

async fn provision_xctest(device_id: String) -> std::io::Result<()> {
    let project = assets::xctest_project("ios-real");
    let team = std::env::var("DEVICE_BOT_IOS_TEAM").ok();
    let identity_hint = std::env::var("DEVICE_BOT_IOS_SIGNING_IDENTITY").ok();
    robot_ios_real::provision_xctest_runner(
        &project,
        &device_id,
        team.as_deref(),
        identity_hint.as_deref(),
    )
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    println!("iOS XCTest runner provisioned for {device_id}");
    Ok(())
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
        .args(["install", "ffmpeg-full"])
        .status()
        .await
        .map_err(|error| {
            std::io::Error::other(format!("could not run {brew} install ffmpeg-full: {error}"))
        })?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "{brew} install ffmpeg-full exited with {}",
            status
                .code()
                .map_or_else(|| "unknown".into(), |code| code.to_string())
        )));
    }
    println!(
        "Recording and video step-caption tools installed. Restart Device Bot to use ffmpeg-full."
    );
    Ok(())
}

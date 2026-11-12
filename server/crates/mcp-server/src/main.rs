use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
};

use device_bot::Server;
use robot_android::AndroidRobot;
use robot_chrome::ChromeRobot;
use robot_core::RobotRegistry;
use robot_ios_real::{IosXCTestRobot, XCTestAgentBridge, XCTestAgentLauncher};
use robot_ios_sim::IosSimulatorRobot;
use robot_macos::MacosRobot;
use robot_windows::WindowsRobot;

mod assets;
mod flavor;
mod launcher;
mod qc;
mod qc_connection;
mod update;

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
    if let Some(home) = std::env::var_os("HOME") {
        for suffix in ["Library/Android/sdk", "Android/Sdk"] {
            let path = PathBuf::from(&home)
                .join(suffix)
                .join("platform-tools")
                .join(executable);
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    executable.into()
}

fn resolve_bearer_token() -> String {
    if let Some(token) = std::env::var("DEVICE_BOT_AUTH")
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
    {
        return token;
    }

    let token = uuid::Uuid::new_v4().simple().to_string();
    eprintln!("DEVICE_BOT_AUTH={token}");
    token
}

fn flutter_app_directory() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("DEVICE_BOT_FLUTTER_APP") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../flutter_app"));
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("flutter_app"));
        candidates.push(current_dir.join("server/flutter_app"));
    }

    candidates
        .into_iter()
        .find(|path| path.join("pubspec.yaml").is_file())
}

fn flutter_device() -> Option<String> {
    if let Some(device) = std::env::var("DEVICE_BOT_FLUTTER_DEVICE")
        .ok()
        .map(|device| device.trim().to_owned())
        .filter(|device| !device.is_empty())
    {
        return Some(device);
    }

    if cfg!(target_os = "macos") {
        return Some("macos".into());
    }
    if cfg!(target_os = "windows") {
        return Some("windows".into());
    }
    if cfg!(target_os = "linux") {
        return Some("linux".into());
    }
    None
}

fn companion_url(address: SocketAddr) -> String {
    let host = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_owned(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    format!("http://{host}:{}", address.port())
}

fn launch_flutter_companion(address: SocketAddr, bearer_token: &str) -> Option<Child> {
    if std::env::var("DEVICE_BOT_FLUTTER_AUTOSTART")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            )
        })
    {
        eprintln!("Flutter companion auto-start disabled by DEVICE_BOT_FLUTTER_AUTOSTART.");
        return None;
    }

    let Some(app_directory) = flutter_app_directory() else {
        eprintln!("Flutter companion source was not found; continuing without auto-start.");
        return None;
    };
    let Some(device) = flutter_device() else {
        eprintln!(
            "Flutter companion has no supported default device; continuing without auto-start."
        );
        return None;
    };
    let flutter_command =
        std::env::var_os("DEVICE_BOT_FLUTTER_COMMAND").unwrap_or_else(|| "flutter".into());
    let server_url = companion_url(address);
    let mut command = Command::new(flutter_command);
    command
        .current_dir(&app_directory)
        .args(["run", "--device-id", &device])
        .arg("--dart-define")
        .arg(format!("DEVICE_BOT_URL={server_url}"))
        .arg("--dart-define")
        .arg(format!("DEVICE_BOT_AUTH={bearer_token}"))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    match command.spawn() {
        Ok(child) => {
            eprintln!(
                "Started Flutter companion from {} for {server_url}.",
                app_directory.display()
            );
            Some(child)
        }
        Err(error) => {
            eprintln!(
                "Could not start Flutter companion with `flutter run`: {error}; continuing without it."
            );
            None
        }
    }
}

fn real_ios_agent_hosts(bind_address: SocketAddr) -> std::io::Result<Option<String>> {
    if bind_address.ip().is_loopback() {
        if std::env::var("DEVICE_BOT_IOS_AGENT_HOSTS")
            .ok()
            .is_some_and(|hosts| !hosts.trim().is_empty())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "DEVICE_BOT_IOS_AGENT_BIND is loopback, so an iPhone cannot open DEVICE_BOT_IOS_AGENT_HOSTS; bind a LAN address or 0.0.0.0",
            ));
        }
        return Ok(None);
    }
    if let Some(hosts) = std::env::var("DEVICE_BOT_IOS_AGENT_HOSTS")
        .ok()
        .filter(|hosts| !hosts.trim().is_empty())
    {
        return Ok(Some(hosts));
    }
    if !bind_address.ip().is_unspecified() {
        return Ok(Some(bind_address.ip().to_string()));
    }
    let hosts = discovered_lan_hosts()?;
    if hosts.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the iOS agent bridge is listening on every interface, but no LAN address was found; set DEVICE_BOT_IOS_AGENT_HOSTS to this Mac's Wi-Fi or Ethernet address",
        ));
    }
    Ok(Some(hosts.join(",")))
}

fn discovered_lan_hosts() -> std::io::Result<Vec<String>> {
    if !cfg!(target_os = "macos") {
        return Ok(Vec::new());
    }
    let listing = std::process::Command::new("ifconfig")
        .arg("-l")
        .output()
        .map_err(|error| {
            std::io::Error::other(format!("could not list network interfaces: {error}"))
        })?;
    if !listing.status.success() {
        return Err(std::io::Error::other(
            "ifconfig -l failed while looking for a LAN address the iPhone can open",
        ));
    }
    let mut hosts = Vec::new();
    for interface in String::from_utf8_lossy(&listing.stdout).split_whitespace() {
        if !interface.starts_with("en") {
            continue;
        }
        let address = std::process::Command::new("ipconfig")
            .args(["getifaddr", interface])
            .output();
        let Ok(address) = address else {
            continue;
        };
        if !address.status.success() {
            continue;
        }
        let address = String::from_utf8_lossy(&address.stdout).trim().to_owned();
        let Ok(ip) = address.parse::<std::net::Ipv4Addr>() else {
            continue;
        };
        if ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() || ip.is_multicast() {
            continue;
        }
        hosts.push(address);
    }
    Ok(hosts)
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

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let mut command = arguments.next();
    match command.as_deref() {
        None => return launcher::start(true),
        Some("qc") => {
            if arguments.next().is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "qc accepts no arguments",
                ));
            }
            return qc::open();
        }
        Some("start") => {
            if arguments.next().is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "start accepts no arguments",
                ));
            }
            return launcher::start(false);
        }
        Some("run") => return launcher::run(arguments),
        Some("--help" | "-h" | "help") => {
            println!("{}", flavor::help());
            return Ok(());
        }
        Some("mcp") => {
            command = arguments.next();
            if matches!(command.as_deref(), Some("--help" | "-h")) {
                println!("device-bot mcp [--listen HOST:PORT]\nDefaults to JSON-RPC over stdio.");
                return Ok(());
            }
        }
        _ => {}
    }
    if matches!(command.as_deref(), Some("--version" | "-v")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--version accepts no arguments",
            ));
        }
        if flavor::FLAVOR == flavor::Flavor::Dev {
            println!("{} dev", env!("CARGO_PKG_VERSION"));
        } else {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        return Ok(());
    }
    if matches!(command.as_deref(), Some("--check-install")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--check-install accepts no arguments",
            ));
        }
        return assets::check_install();
    }
    if matches!(command.as_deref(), Some("--health")) {
        let device_id = arguments
            .next()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "--health requires exactly one Simulator UDID or physical iPhone ID",
                )
            })?;
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--health requires exactly one Simulator UDID or physical iPhone ID",
            ));
        }
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
        return Ok(());
    }
    if matches!(command.as_deref(), Some("--check-update")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--check-update accepts no arguments",
            ));
        }
        match update::check_latest_release(env!("CARGO_PKG_VERSION"))? {
            Some(update) => {
                let encoded = serde_json::to_string_pretty(&update).map_err(|error| {
                    std::io::Error::other(format!("encode update information: {error}"))
                })?;
                println!("{encoded}");
            }
            None => println!("device-bot {} is up to date.", env!("CARGO_PKG_VERSION")),
        }
        return Ok(());
    }
    if matches!(command.as_deref(), Some("--upgrade" | "-u")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--upgrade accepts no arguments",
            ));
        }
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
        return Ok(());
    }
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
        if robot_macos::request_screen_recording_permission() {
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
        let identity_hint = std::env::var("DEVICE_BOT_IOS_SIGNING_IDENTITY").ok();
        let team = robot_ios_real::apple_development_team(identity_hint.as_deref())
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        println!("{team}");
        return Ok(());
    }
    if matches!(command.as_deref(), Some("--configure-ios-signing")) {
        if arguments.next().is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--configure-ios-signing accepts no arguments; use DEVICE_BOT_IOS_TEAM to select a team",
            ));
        }
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
        return Ok(());
    }
    let listen_address: Option<SocketAddr> = match command.as_deref() {
        None => None,
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
            Some(address.parse().map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid listen address: {error}"),
                )
            })?)
        }
        Some(argument) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown argument: {argument}; see device-bot --help"),
            ));
        }
    };
    let adb_path = android_adb_path();
    let xcrun_path = std::env::var("XCRUN_PATH").unwrap_or_else(|_| "xcrun".into());
    let artifact_dir =
        std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into());
    let ios_agent_address = std::env::var("DEVICE_BOT_IOS_AGENT_BIND")
        .unwrap_or_else(|_| format!("127.0.0.1:{}", flavor::FLAVOR.ios_agent_port()));
    let ios_agent_address: SocketAddr = ios_agent_address.parse().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid DEVICE_BOT_IOS_AGENT_BIND address: {error}"),
        )
    })?;
    let ios_agent_token = std::env::var("DEVICE_BOT_IOS_AGENT_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty());
    if !ios_agent_address.ip().is_loopback() && ios_agent_token.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "DEVICE_BOT_IOS_AGENT_TOKEN is required when the iOS agent bridge is reachable beyond loopback",
        ));
    }
    let ios_agent_hosts = real_ios_agent_hosts(ios_agent_address)?;
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
    let ios_sim_agent_address = std::env::var("DEVICE_BOT_IOS_SIM_AGENT_BIND")
        .unwrap_or_else(|_| format!("127.0.0.1:{}", flavor::FLAVOR.ios_sim_agent_port()));
    let ios_sim_agent_address: SocketAddr = ios_sim_agent_address.parse().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid DEVICE_BOT_IOS_SIM_AGENT_BIND address: {error}"),
        )
    })?;
    let ios_sim_agent =
        XCTestAgentBridge::new(std::env::var("DEVICE_BOT_IOS_SIM_AGENT_TOKEN").ok());
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
            let bearer_token = resolve_bearer_token();
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
            let result = axum::serve(
                listener,
                device_bot::http::router(robot, Some(bearer_token)),
            )
            .await;
            if let Some(mut child) = flutter_companion.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            result
        }
    }
}

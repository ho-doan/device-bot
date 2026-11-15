//! Optional Flutter companion process lifecycle.

use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::{Child, Command, Stdio},
};

pub(super) fn flutter_app_directory() -> Option<PathBuf> {
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

pub(super) fn launch_flutter_companion(address: SocketAddr, bearer_token: &str) -> Option<Child> {
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

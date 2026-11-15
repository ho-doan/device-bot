//! Resolve and validate server configuration before binding bridge listeners.

use crate::flavor;
use std::{net::SocketAddr, path::PathBuf};

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

pub(super) fn resolve_bearer_token() -> String {
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

pub(super) fn discovered_lan_hosts() -> std::io::Result<Vec<String>> {
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

pub(super) struct ServerConfig {
    pub adb_path: String,
    pub xcrun_path: String,
    pub artifact_dir: String,
    pub ios_agent_address: SocketAddr,
    pub ios_agent_token: Option<String>,
    pub ios_agent_hosts: Option<String>,
    pub ios_sim_agent_address: SocketAddr,
    pub ios_sim_agent_token: Option<String>,
}

impl ServerConfig {
    pub fn from_env() -> std::io::Result<Self> {
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
        let ios_sim_agent_address = std::env::var("DEVICE_BOT_IOS_SIM_AGENT_BIND")
            .unwrap_or_else(|_| format!("127.0.0.1:{}", flavor::FLAVOR.ios_sim_agent_port()));
        let ios_sim_agent_address: SocketAddr = ios_sim_agent_address.parse().map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid DEVICE_BOT_IOS_SIM_AGENT_BIND address: {error}"),
            )
        })?;
        Ok(Self {
            adb_path,
            xcrun_path,
            artifact_dir,
            ios_agent_address,
            ios_agent_token,
            ios_agent_hosts,
            ios_sim_agent_address,
            ios_sim_agent_token: std::env::var("DEVICE_BOT_IOS_SIM_AGENT_TOKEN").ok(),
        })
    }
}

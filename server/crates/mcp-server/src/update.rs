//! Release checking and self-upgrade support for the released MCP server.
//!
//! The flow intentionally follows my-cleaner's update contract: check the
//! latest GitHub release, compare numeric versions, wait until the release
//! artifact has a verified checksum, then rerun the official installer. The
//! server never upgrades in the background while it is serving MCP requests.

use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/ho-doan/device-bot/releases/latest";
const INSTALL_SCRIPT_MACOS: &str =
    "https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.sh";
const INSTALL_SCRIPT_WINDOWS: &str =
    "https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.ps1";
const RELEASE_CHECKSUMS: &str =
    "https://github.com/ho-doan/device-bot/releases/download/v{version}/checksums.txt";
const INSTALLER_READY_ATTEMPTS: usize = 6;
const BINARY_NAME: &str = "mobile-mcp-server";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// Return the latest release only after its platform archive is checksum-ready.
pub fn check_latest_release(current_version: &str) -> io::Result<Option<UpdateInfo>> {
    let release: GithubRelease = serde_json::from_str(&fetch_url(RELEASES_API)?)
        .map_err(|error| io::Error::other(format!("parse GitHub latest release: {error}")))?;
    let latest_version = normalize_version(&release.tag_name).ok_or_else(|| {
        io::Error::other("GitHub latest release has an invalid numeric version tag")
    })?;
    let current_version = normalize_version(current_version)
        .ok_or_else(|| io::Error::other("mobile-mcp-server has an invalid package version"))?;

    if compare_versions(&latest_version, &current_version).is_gt() {
        wait_for_installer_ready(&latest_version)?;
        Ok(Some(UpdateInfo {
            current_version,
            latest_version,
            release_url: release.html_url,
        }))
    } else {
        Ok(None)
    }
}

/// Upgrade a released binary through the official installer.
pub fn perform_upgrade(update: &UpdateInfo) -> io::Result<()> {
    let executable = std::env::current_exe()
        .map_err(|error| io::Error::other(format!("locate running executable: {error}")))?;
    let executable = executable.canonicalize().unwrap_or(executable);

    if executable.components().any(|component| {
        component
            .as_os_str()
            .eq_ignore_ascii_case(std::ffi::OsStr::new("target"))
    }) {
        return Err(io::Error::other(
            "cannot upgrade a development build; install mobile-mcp-server with the release installer",
        ));
    }

    let install_dir = executable.parent().ok_or_else(|| {
        io::Error::other("running mobile-mcp-server has no parent installation directory")
    })?;
    let installer = wait_for_installer_script(&update.latest_version)?;
    run_installer_script(&installer, &update.latest_version, install_dir)
}

fn wait_for_installer_ready(version: &str) -> io::Result<()> {
    let target = current_release_target()?;
    for attempt in 1..=INSTALLER_READY_ATTEMPTS {
        match installer_is_ready(version, target, attempt) {
            Ok(true) => {
                if attempt > 1 {
                    eprintln!("Device Bot release metadata is ready for {version}.");
                }
                return Ok(());
            }
            Ok(false) => eprintln!(
                "Waiting for Device Bot {version} checksum metadata ({attempt}/{INSTALLER_READY_ATTEMPTS})..."
            ),
            Err(error) => eprintln!(
                "Device Bot release metadata is not ready ({attempt}/{INSTALLER_READY_ATTEMPTS}): {error}"
            ),
        }

        if attempt < INSTALLER_READY_ATTEMPTS {
            sleep(Duration::from_secs(
                2_u64.pow((attempt.saturating_sub(1).min(3)) as u32),
            ));
        }
    }

    Err(io::Error::other(format!(
        "Device Bot release {version} did not publish a verified checksum for {target} after {INSTALLER_READY_ATTEMPTS} attempts"
    )))
}

fn wait_for_installer_script(version: &str) -> io::Result<String> {
    let target = current_release_target()?;
    for attempt in 1..=INSTALLER_READY_ATTEMPTS {
        let installer_url = format!("{}?version={version}&attempt={attempt}", installer_url());
        match fetch_url(&installer_url) {
            Ok(script) => {
                if installer_is_ready_with_script(version, target, attempt, &script)? {
                    return Ok(script);
                }
            }
            Err(error) => eprintln!(
                "Device Bot installer is not ready ({attempt}/{INSTALLER_READY_ATTEMPTS}): {error}"
            ),
        }

        if attempt < INSTALLER_READY_ATTEMPTS {
            sleep(Duration::from_secs(
                2_u64.pow((attempt.saturating_sub(1).min(3)) as u32),
            ));
        }
    }

    Err(io::Error::other(format!(
        "Device Bot installer did not publish a verified checksum for {version} ({target}) after {INSTALLER_READY_ATTEMPTS} attempts"
    )))
}

fn installer_is_ready(version: &str, target: &str, attempt: usize) -> io::Result<bool> {
    let installer_url = format!("{}?version={version}&attempt={attempt}", installer_url());
    let script = fetch_url(&installer_url)?;
    installer_is_ready_with_script(version, target, attempt, &script)
}

fn installer_is_ready_with_script(
    version: &str,
    target: &str,
    attempt: usize,
    script: &str,
) -> io::Result<bool> {
    if script.trim().is_empty() || !script.contains(BINARY_NAME) {
        return Ok(false);
    }

    let archive = archive_name(version, target);
    let manifest_url = format!(
        "{}?version={version}&attempt={attempt}",
        RELEASE_CHECKSUMS.replace("{version}", version)
    );
    let manifest = fetch_url(&manifest_url)?;
    Ok(checksum_manifest_has_archive(&manifest, &archive))
}

fn installer_url() -> &'static str {
    if cfg!(target_os = "windows") {
        INSTALL_SCRIPT_WINDOWS
    } else {
        INSTALL_SCRIPT_MACOS
    }
}

fn archive_name(version: &str, target: &str) -> String {
    let extension = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("{BINARY_NAME}-v{version}-{target}.{extension}")
}

fn current_release_target() -> io::Result<&'static str> {
    if cfg!(target_os = "macos") {
        return match std::env::consts::ARCH {
            "aarch64" => Ok("aarch64-apple-darwin"),
            "x86_64" => Ok("x86_64-apple-darwin"),
            architecture => Err(io::Error::other(format!(
                "unsupported macOS architecture for upgrade: {architecture}"
            ))),
        };
    }

    if cfg!(target_os = "windows") {
        return Ok("x86_64-pc-windows-msvc");
    }

    Err(io::Error::other(
        "automatic upgrade is supported only on macOS and Windows",
    ))
}

#[cfg(target_os = "windows")]
fn fetch_url(url: &str) -> io::Result<String> {
    let escaped_url = url.replace('\'', "''");
    let command = format!(
        "$ProgressPreference='SilentlyContinue'; (Invoke-WebRequest -UseBasicParsing -Headers @{{'Cache-Control'='no-cache'; Pragma='no-cache'}} -Uri '{escaped_url}').Content"
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command.as_str(),
        ])
        .output()
        .map_err(|error| io::Error::other(format!("start PowerShell network request: {error}")))?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "PowerShell network request exited with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(target_os = "windows"))]
fn fetch_url(url: &str) -> io::Result<String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "15",
            "--retry",
            "2",
            "--proto",
            "=https",
            "--tlsv1.2",
            "-H",
            "Cache-Control: no-cache",
            "-H",
            "Pragma: no-cache",
            url,
        ])
        .output()
        .map_err(|error| io::Error::other(format!("start curl network request: {error}")))?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "curl network request exited with {}",
            output.status
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| io::Error::other(format!("network response was not UTF-8: {error}")))
}

#[cfg(target_os = "macos")]
fn run_installer_script(script: &str, version: &str, install_dir: &Path) -> io::Result<()> {
    let mut child = Command::new("sh")
        .arg("-s")
        .stdin(Stdio::piped())
        .env("DEVICE_BOT_VERSION", version)
        .env("DEVICE_BOT_INSTALL_DIR", install_dir)
        .spawn()
        .map_err(|error| io::Error::other(format!("start verified macOS installer: {error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("open stdin for macOS installer"))?;
    stdin
        .write_all(script.as_bytes())
        .map_err(|error| io::Error::other(format!("send macOS installer script: {error}")))?;
    drop(stdin);
    let status = child
        .wait()
        .map_err(|error| io::Error::other(format!("wait for macOS installer: {error}")))?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "macOS installer exited with {status}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_installer_script(script: &str, version: &str, install_dir: &Path) -> io::Result<()> {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::other(format!("clock error: {error}")))?
        .as_nanos();
    let script_path = std::env::temp_dir().join(format!(
        "device-bot-upgrade-{}-{stamp}.ps1",
        std::process::id()
    ));
    fs::write(&script_path, script)
        .map_err(|error| io::Error::other(format!("stage PowerShell installer: {error}")))?;

    let child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .env("DEVICE_BOT_VERSION", version)
        .env("DEVICE_BOT_INSTALL_DIR", install_dir)
        .env("DEVICE_BOT_WAIT_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| io::Error::other(format!("start deferred Windows installer: {error}")))?;
    drop(child);
    eprintln!("Windows installer scheduled; the server will exit before replacing its executable.");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_installer_script(_script: &str, _version: &str, _install_dir: &Path) -> io::Result<()> {
    Err(io::Error::other(
        "automatic upgrade is supported only on macOS and Windows",
    ))
}

fn normalize_version(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|part| part.parse::<u64>().is_err()) {
        return None;
    }
    Some(parts.join("."))
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect::<Vec<_>>();
    let right = right
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect::<Vec<_>>();
    let length = left.len().max(right.len());
    (0..length)
        .map(|index| {
            (
                left.get(index).copied().unwrap_or_default(),
                right.get(index).copied().unwrap_or_default(),
            )
        })
        .find_map(|(left, right)| (left != right).then_some(left.cmp(&right)))
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn checksum_manifest_has_archive(manifest: &str, archive: &str) -> bool {
    manifest.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let Some(checksum) = parts.next() else {
            return false;
        };
        let Some(name) = parts.next_back() else {
            return false;
        };
        checksum.len() == 64
            && checksum
                .chars()
                .all(|character| character.is_ascii_hexdigit())
            && name.trim_start_matches('*') == archive
    })
}

#[cfg(test)]
mod tests {
    use super::{archive_name, checksum_manifest_has_archive, compare_versions, normalize_version};
    use std::cmp::Ordering;

    #[test]
    fn normalizes_release_versions() {
        assert_eq!(normalize_version("v0.1.3").as_deref(), Some("0.1.3"));
        assert!(normalize_version("latest").is_none());
    }

    #[test]
    fn compares_release_versions_without_semver_dependency() {
        assert_eq!(compare_versions("0.1.4", "0.1.3"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.3", "0.1.3"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "0.99.9"), Ordering::Greater);
    }

    #[test]
    fn checksum_manifest_requires_a_verified_archive_entry() {
        let manifest = "abc\n1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef  mobile-mcp-server-v0.1.6-x86_64-pc-windows-msvc.zip\n";
        assert!(checksum_manifest_has_archive(
            manifest,
            "mobile-mcp-server-v0.1.6-x86_64-pc-windows-msvc.zip"
        ));
        assert!(!checksum_manifest_has_archive(
            manifest,
            "mobile-mcp-server-v0.1.6-aarch64-pc-windows-msvc.zip"
        ));
    }

    #[test]
    fn release_archive_names_match_the_installer_contract() {
        let extension = if cfg!(target_os = "windows") {
            "zip"
        } else {
            "tar.gz"
        };
        assert_eq!(
            archive_name("0.1.0", "aarch64-apple-darwin"),
            format!("mobile-mcp-server-v0.1.0-aarch64-apple-darwin.{extension}")
        );
    }
}

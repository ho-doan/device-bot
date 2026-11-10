//! Bounded, read-only iPhone connection and Xcode destination probes.
use quick_xml::{Reader, events::Event};
use serde::Serialize;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCheck {
    pub name: &'static str,
    pub status: &'static str,
    pub detail: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConnectionReport {
    pub device_id: String,
    pub connected: bool,
    pub usb_attached: Option<bool>,
    pub xcode_destination_ready: Option<bool>,
    pub checks: Vec<ConnectionCheck>,
    pub recommended_actions: Vec<&'static str>,
}
impl DeviceConnectionReport {
    pub fn can_install(&self) -> bool {
        self.connected && self.xcode_destination_ready == Some(true)
    }
    pub fn summary(&self) -> String {
        self.checks
            .iter()
            .filter(|c| c.status == "fail" || c.status == "unknown")
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// A paired Wi-Fi device can have an idle tunnel in `list devices`. Requesting
/// details opens that tunnel, but a successful command may still return cached
/// details: only a connected, paired result for this exact UDID is live evidence.
pub async fn coredevice_connected(xcrun: &str, device: &str) -> Result<bool, String> {
    let path = std::env::temp_dir().join(format!("device-bot-live-{}.json", uuid::Uuid::new_v4()));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let output = Command::new(xcrun)
            .args([
                "devicectl",
                "device",
                "info",
                "details",
                "--device",
                device,
                "--timeout",
                "5",
                "--json-output",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| "CoreDevice live connection probe could not start".to_owned())?;
        if !output.status.success() {
            return Err("CoreDevice could not establish a live USB or network connection".into());
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| "CoreDevice live connection JSON is unavailable".to_owned())?;
        details_confirm_connection(&bytes, device)
    })
    .await
    .map_err(|_| "CoreDevice live connection probe timed out after 3s".to_owned());
    let _ = tokio::fs::remove_file(path).await;
    result?
}

fn details_confirm_connection(bytes: &[u8], device: &str) -> Result<bool, String> {
    let details: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "CoreDevice live connection JSON is invalid".to_owned())?;
    let result = &details["result"];
    Ok(details["info"]["outcome"] == "success"
        && result["hardwareProperties"]["udid"]
            .as_str()
            .is_some_and(|id| id.eq_ignore_ascii_case(device))
        && result["connectionProperties"]["pairingState"] == "paired"
        && result["connectionProperties"]["tunnelState"] == "connected")
}

pub async fn usb_attached(device: &str) -> Result<bool, String> {
    tokio::time::timeout(Duration::from_secs(4), async {
        let output = Command::new("/usr/sbin/ioreg")
            .args(["-r", "-c", "IOUSBHostDevice", "-a"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| "USB registry probe unavailable")?;
        if !output.status.success() {
            return Err("USB registry probe failed".into());
        }
        let xml =
            std::str::from_utf8(&output.stdout).map_err(|_| "USB registry returned invalid XML")?;
        usb_registry_contains_device(xml, device)
    })
    .await
    .map_err(|_| "USB registry probe timed out".to_owned())?
}

/// Query the current lock state independently of the XCTest helper. Missing,
/// failed or timed-out probes are unknown, never evidence that a phone is locked.
pub async fn device_lock_state(xcrun: &str, device: &str) -> Option<bool> {
    let path = std::env::temp_dir().join(format!("device-bot-lock-{}.json", uuid::Uuid::new_v4()));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let output = Command::new(xcrun)
            .args([
                "devicectl",
                "device",
                "info",
                "lockState",
                "--device",
                device,
                "--timeout",
                "5",
                "--json-output",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let bytes = tokio::fs::read(&path).await.ok()?;
        parse_device_lock_state(&bytes)
    })
    .await
    .ok()
    .flatten();
    let _ = tokio::fs::remove_file(path).await;
    result
}

fn parse_device_lock_state(bytes: &[u8]) -> Option<bool> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if value["info"]["outcome"] != "success" {
        return None;
    }
    value["result"]["passcodeRequired"].as_bool()
}

fn normalized_id(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}
pub fn usb_registry_contains_device(xml: &str, device: &str) -> Result<bool, String> {
    let wanted = normalized_id(device);
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut key = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"key" => {
                key = reader
                    .read_text(element.name())
                    .map_err(|_| "USB registry key is invalid")?
                    .into_owned();
            }
            Ok(Event::Start(element)) if element.name().as_ref() == b"string" => {
                let value = reader
                    .read_text(element.name())
                    .map_err(|_| "USB registry value is invalid")?;
                if ["USB Serial Number", "kUSBSerialNumberString"].contains(&key.as_str())
                    && !wanted.is_empty()
                    && normalized_id(&value) == wanted
                {
                    return Ok(true);
                }
                key.clear();
            }
            Ok(Event::Eof) => return Ok(false),
            Err(_) => return Err("USB registry returned invalid XML".into()),
            _ => {}
        }
    }
}

pub async fn xcode_destination_ready(
    xcodebuild: &str,
    project: &Path,
    device: &str,
) -> Result<bool, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(12),
        Command::new(xcodebuild)
            .args(["-showdestinations", "-project"])
            .arg(project)
            .args(["-scheme", "MobileMcpRealRunner"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Xcode destination check timed out")?
    .map_err(|_| "Xcode destination check could not start")?;
    if !output.status.success() {
        return Err("Xcode could not resolve the helper project/destinations".into());
    }
    Ok(destination_is_eligible(
        &String::from_utf8_lossy(&output.stdout),
        device,
    ))
}
pub fn destination_is_eligible(output: &str, device: &str) -> bool {
    let mut eligible = false;
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("available destinations") {
            eligible = true;
        }
        if lower.contains("ineligible destinations")
            || lower.contains("incompatible destinations")
            || lower.contains("unavailable destinations")
        {
            eligible = false;
        }
        if eligible
            && line.contains("platform:iOS,")
            && line.contains(&format!("id:{device},"))
            && !lower.contains("error:")
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_state_is_current_and_missing_data_is_unknown() {
        for locked in [true, false] {
            let value = serde_json::json!({"info":{"outcome":"success"},"result":{
                "passcodeRequired":locked,"unlockedSinceBoot":true}});
            assert_eq!(
                parse_device_lock_state(&serde_json::to_vec(&value).unwrap()),
                Some(locked)
            );
        }
        for bytes in [
            b"{}".as_slice(),
            b"invalid",
            br#"{"info":{"outcome":"failure"},"result":{"passcodeRequired":true}}"#,
            br#"{"info":{"outcome":"success"},"result":{"unlockedSinceBoot":true}}"#,
        ] {
            assert_eq!(parse_device_lock_state(bytes), None);
        }
    }
    #[test]
    fn cached_success_does_not_prove_a_live_connection() {
        let device = "00008130-000959E43EDA001C";
        let live = serde_json::json!({"info":{"outcome":"success"},"result":{
            "hardwareProperties":{"udid":device},
            "connectionProperties":{"tunnelState":"connected","pairingState":"paired"}
        }});
        assert!(details_confirm_connection(&serde_json::to_vec(&live).unwrap(), device).unwrap());
        for (path, value) in [
            (
                vec!["result", "connectionProperties", "tunnelState"],
                "disconnected",
            ),
            (
                vec!["result", "connectionProperties", "tunnelState"],
                "unavailable",
            ),
            (
                vec!["result", "connectionProperties", "tunnelState"],
                "connecting",
            ),
            (
                vec!["result", "connectionProperties", "pairingState"],
                "unpaired",
            ),
            (
                vec!["result", "hardwareProperties", "udid"],
                "another-device",
            ),
            (vec!["info", "outcome"], "failure"),
        ] {
            let mut cached = live.clone();
            let mut field = &mut cached;
            for key in path {
                field = &mut field[key];
            }
            *field = serde_json::json!(value);
            assert!(
                !details_confirm_connection(&serde_json::to_vec(&cached).unwrap(), device).unwrap(),
                "{cached}"
            );
        }
        assert!(!details_confirm_connection(b"{}", device).unwrap());
        assert!(details_confirm_connection(b"invalid json", device).is_err());
    }

    #[test]
    fn usb_match_uses_exact_udid_and_accepts_binary_data_fields() {
        let xml = r#"<?xml version="1.0"?><plist><array><dict><key>IORegistryEntryChildren</key><array><dict><key>USB Product Name</key><string>iPhone</string><key>descriptor</key><data>AQID</data><key>USB Serial Number</key><string>00008130000959E43EDA001C</string></dict></array></dict></array></plist>"#;
        assert!(usb_registry_contains_device(xml, "00008130-000959E43EDA001C").unwrap());
        assert!(!usb_registry_contains_device(xml, "00008130-OTHER").unwrap());
        assert!(
            !usb_registry_contains_device("<plist><array/></plist>", "00008130-000959E43EDA001C")
                .unwrap()
        );
    }
    #[test]
    fn cached_or_ineligible_xcode_destination_is_not_ready_for_install() {
        let unavailable = "Available destinations for scheme X:\n{ platform:iOS, id:placeholder, name:Any iOS Device }\nIneligible destinations for scheme X:\n{ platform:iOS, id:phone, name:iPhone, error:locked }";
        assert!(!destination_is_eligible(unavailable, "phone"));
        assert!(destination_is_eligible(
            "Available destinations for scheme X:\n{ platform:iOS, arch:arm64, id:phone, name:iPhone }",
            "phone"
        ));
    }
}

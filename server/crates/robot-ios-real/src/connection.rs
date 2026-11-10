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

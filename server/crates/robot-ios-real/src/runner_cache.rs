//! Versioned build products and live, device-specific runner readiness.
use robot_core::{RobotError, UiRunnerStatus};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct RunnerCache<'a> {
    pub project: &'a Path,
    pub derived_data: &'a Path,
    pub simulator: bool,
}
impl RunnerCache<'_> {
    pub fn version(&self) -> Result<String, RobotError> {
        if !self.project.join("project.pbxproj").is_file() {
            return Err(RobotError::InvalidInput(format!(
                "XCTest project assets missing: {}",
                self.project.display()
            )));
        }
        let root = self
            .project
            .parent()
            .ok_or_else(|| RobotError::InvalidInput("runner project has no parent".into()))?;
        let mut files = Vec::new();
        source_files(root, root, &mut files).map_err(io_error)?;
        files.sort();
        if files.is_empty() {
            return Err(RobotError::InvalidInput(
                "runner sources are missing".into(),
            ));
        }
        // Deterministic cache identity, not a cryptographic integrity check.
        let mut hash = 0xcbf29ce484222325u64;
        for file in files {
            for byte in file
                .to_string_lossy()
                .bytes()
                .chain([0])
                .chain(source_content(&root.join(&file)).map_err(io_error)?)
            {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        Ok(format!("runner-v2-{hash:016x}"))
    }
    pub fn products_ready(&self) -> bool {
        let (platform, host, runner) = if self.simulator {
            (
                "Debug-iphonesimulator",
                "MobileMcpSimulatorRunnerHost.app",
                "MobileMcpSimulatorRunnerUITests-Runner.app",
            )
        } else {
            (
                "Debug-iphoneos",
                "MobileMcpRealRunnerHost.app",
                "MobileMcpRealRunnerUITests-Runner.app",
            )
        };
        let products = self.derived_data.join("Build/Products").join(platform);
        products.join(host).is_dir() && products.join(runner).is_dir()
    }
    pub fn is_current(&self, version: &str) -> bool {
        self.products_ready()
            && fs::read_to_string(self.derived_data.join("device-bot-runner.version"))
                .is_ok_and(|v| v == version)
    }
    pub fn mark_current(&self, version: &str) -> Result<(), RobotError> {
        if !self.products_ready() {
            return Err(RobotError::CommandFailed(
                "XCTest build did not produce both runner apps".into(),
            ));
        }
        fs::write(self.derived_data.join("device-bot-runner.version"), version).map_err(io_error)
    }
}
fn io_error(error: std::io::Error) -> RobotError {
    RobotError::CommandFailed(format!("XCTest runner cache: {error}"))
}

fn source_content(path: &Path) -> std::io::Result<Vec<u8>> {
    // Xcode can rewrite plist/project indentation without changing the build.
    // Hash their parsed values, so opening a project does not invalidate cache.
    #[cfg(target_os = "macos")]
    if matches!(
        path.extension().and_then(|v| v.to_str()),
        Some("plist" | "pbxproj")
    ) {
        if let Ok(output) = std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(path)
            .output()
        {
            if output.status.success() {
                if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) {
                    return Ok(serde_json::to_vec(&value).unwrap());
                }
            }
        }
    }
    fs::read(path)
}
fn source_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if entry.file_type()?.is_dir() {
            if !matches!(
                name.to_str(),
                Some("build" | ".build" | ".git" | "DerivedData" | "xcuserdata")
            ) {
                source_files(root, &path, files)?;
            }
        } else if matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("swift" | "pbxproj" | "plist" | "xcscheme" | "xcconfig" | "entitlements")
        ) {
            files.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

pub fn live_status(
    device_id: &str,
    expected: Option<String>,
    cache_ready: bool,
    health: Result<Value, RobotError>,
) -> UiRunnerStatus {
    let mut status = UiRunnerStatus {
        expected_version: expected,
        cache_ready,
        state: "disconnected".into(),
        ..Default::default()
    };
    match health {
        Err(error) => status.detail = error.to_string(),
        Ok(health) => {
            status.agent_version = health["agentVersion"].as_str().map(str::to_owned);
            status.target_app_id = health["bundleId"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            if health["deviceId"].as_str() != Some(device_id) {
                status.state = "wrong_device".into();
                status.detail = "Helper identity is missing or belongs to another device; prepare this device's runner".into();
            } else if status.expected_version.is_none()
                || status.agent_version != status.expected_version
            {
                status.state = "version_mismatch".into();
                status.detail = "Helper version is unknown or outdated; prepare to build/install the current version".into();
            } else if status.target_app_id.is_none() {
                status.state = "needs_target".into();
                status.detail =
                    "Helper connected and current; select and launch the target app".into();
            } else if health["appForeground"] != true {
                status.state = "target_not_running".into();
                status.detail = "Helper is current but the selected app is not in the foreground; launch or prepare the target app".into();
            } else {
                status.ready = true;
                status.state = "ready".into();
                status.detail =
                    "Current helper confirmed a live heartbeat for this device and target app"
                        .into();
            }
        }
    }
    status
}

pub fn compatible(health: &Value, device: &str, version: &str) -> bool {
    health["deviceId"].as_str() == Some(device) && health["agentVersion"].as_str() == Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn build_products_alone_never_prove_control_readiness() {
        let s = live_status(
            "sim",
            Some("v2".into()),
            true,
            Err(RobotError::CommandFailed("no heartbeat".into())),
        );
        assert!(!s.ready);
        assert!(s.cache_ready);
        for health in [
            json!({"deviceId":"other", "agentVersion":"v2", "bundleId":"app", "appForeground":true}),
            json!({"deviceId":"sim", "agentVersion":"v1", "bundleId":"app"}),
            json!({"deviceId":"sim", "agentVersion":"v2", "bundleId":"", "appForeground":false}),
            json!({"deviceId":"sim", "agentVersion":"v2", "bundleId":"app", "appForeground":false}),
            json!({"deviceId":"sim", "agentVersion":"v2", "bundleId":"app"}),
        ] {
            assert!(!live_status("sim", Some("v2".into()), true, Ok(health)).ready);
        }
        assert!(
            live_status(
                "sim",
                Some("v2".into()),
                false,
                Ok(json!({"deviceId":"sim", "agentVersion":"v2", "bundleId":"app", "appForeground":true}))
            )
            .ready
        );
    }
    #[test]
    fn cache_invalidates_source_changes_but_not_logs_and_reuses_same_sources() {
        let root = std::env::temp_dir().join(format!("runner-cache-{}", uuid::Uuid::new_v4()));
        let project = root.join("Runner.xcodeproj");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("project.pbxproj"), "project").unwrap();
        fs::write(root.join("Runner.swift"), "version one").unwrap();
        let derived = root.join("build");
        let products = derived.join("Build/Products/Debug-iphonesimulator");
        for name in [
            "MobileMcpSimulatorRunnerHost.app",
            "MobileMcpSimulatorRunnerUITests-Runner.app",
        ] {
            fs::create_dir_all(products.join(name)).unwrap();
        }
        let cache = RunnerCache {
            project: &project,
            derived_data: &derived,
            simulator: true,
        };
        let version = cache.version().unwrap();
        assert!(!cache.is_current(&version));
        cache.mark_current(&version).unwrap();
        assert!(cache.is_current(&version));
        fs::write(derived.join("test.log"), "run log").unwrap();
        assert_eq!(version, cache.version().unwrap());
        fs::write(root.join("Runner.swift"), "version two").unwrap();
        assert_ne!(version, cache.version().unwrap());
        assert!(!cache.is_current(&cache.version().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn xcode_plist_reformatting_does_not_change_build_identity() {
        let root = std::env::temp_dir().join(format!("runner-plist-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("Info.plist");
        fs::write(&file, r#"<?xml version="1.0"?><plist version="1.0"><dict><key>version</key><string>one</string></dict></plist>"#).unwrap();
        let first = source_content(&file).unwrap();
        fs::write(&file, "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n  <key>version</key>\n  <string>one</string>\n</dict>\n</plist>").unwrap();
        assert_eq!(first, source_content(&file).unwrap());
        fs::write(&file, r#"<?xml version="1.0"?><plist version="1.0"><dict><key>version</key><string>two</string></dict></plist>"#).unwrap();
        assert_ne!(first, source_content(&file).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}

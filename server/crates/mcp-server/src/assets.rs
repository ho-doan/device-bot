//! Locate versioned XCTest projects shipped beside a released executable.
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn installed_root(executable: &Path) -> PathBuf {
    executable
        .parent()
        .unwrap_or(Path::new("."))
        .join("device-bot-assets")
        .join(VERSION)
}

fn relative_project(platform: &str) -> PathBuf {
    let name = match platform {
        "ios-sim" => "MobileMcpSimulatorRunner.xcodeproj",
        "ios-real" => "MobileMcpRealRunner.xcodeproj",
        _ => unreachable!("only XCTest platforms have bundled projects"),
    };
    PathBuf::from(platform).join("xctest-runner").join(name)
}

pub fn xctest_project(platform: &str) -> PathBuf {
    if let Some(root) = std::env::var_os("DEVICE_BOT_ASSET_DIR") {
        return PathBuf::from(root).join(relative_project(platform));
    }
    let installed = installed_root(&std::env::current_exe().unwrap_or_default())
        .join(relative_project(platform));
    if installed.join("project.pbxproj").is_file() {
        return installed;
    }
    // Keep source-tree builds working, without relying on this path in releases.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../robot-{platform}/xctest-runner"))
        .join(relative_project(platform).file_name().unwrap());
    if source.join("project.pbxproj").is_file() {
        source
    } else {
        installed
    }
}

pub fn check_install() -> std::io::Result<()> {
    let executable = std::env::current_exe()?;
    let root = std::env::var_os("DEVICE_BOT_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| installed_root(&executable));
    let mut missing = Vec::new();
    let mut problems = Vec::new();
    if cfg!(target_os = "macos") {
        match std::fs::read(root.join("manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        {
            Some(manifest) if manifest["version"] == VERSION => {
                match manifest["files"]
                    .as_array()
                    .filter(|files| !files.is_empty())
                {
                    Some(files) => {
                        for name in files {
                            if let Some(name) = name.as_str() {
                                let relative = Path::new(name);
                                if !relative
                                    .components()
                                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                                {
                                    problems.push("invalid asset manifest path".to_owned());
                                } else if !root.join(relative).is_file() {
                                    missing.push(root.join(relative));
                                }
                            } else {
                                problems.push("invalid asset manifest entry".to_owned());
                            }
                        }
                    }
                    None => problems.push("asset manifest contains no files".to_owned()),
                }
            }
            _ => problems.push("missing or mismatched asset manifest".to_owned()),
        }
        for platform in ["ios-sim", "ios-real"] {
            let project = root.join(relative_project(platform));
            for file in [
                project.join("project.pbxproj"),
                project
                    .join("xcshareddata/xcschemes")
                    .join(if platform == "ios-sim" {
                        "MobileMcpSimulatorRunner.xcscheme"
                    } else {
                        "MobileMcpRealRunner.xcscheme"
                    }),
            ] {
                if !file.is_file() {
                    missing.push(file);
                }
            }
            let tests = root.join(platform).join("xctest-runner/RunnerTests");
            let files: &[&str] = if platform == "ios-sim" {
                &[
                    "MobileMcpSimulatorRunnerUITests.swift",
                    "SimulatorXCTestDriver.swift",
                    "SimulatorXCTestHostAgent.swift",
                ]
            } else {
                &["MobileMcpRealRunnerUITests.swift"]
            };
            for file in files {
                if !tests.join(file).is_file() {
                    missing.push(tests.join(file));
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "version": VERSION, "executable": executable, "assetDir": root,
            "valid": missing.is_empty() && problems.is_empty(), "missing": missing, "problems": problems
        })
    );
    if missing.is_empty() && problems.is_empty() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "release XCTest assets are missing; reinstall this version",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_assets_follow_the_executable_and_package_version() {
        assert_eq!(
            installed_root(Path::new("/tmp/clean/bin/device-bot")),
            PathBuf::from("/tmp/clean/bin/device-bot-assets").join(VERSION)
        );
    }
    #[test]
    fn bundled_projects_have_platform_specific_relative_paths() {
        assert_eq!(
            relative_project("ios-sim"),
            PathBuf::from("ios-sim/xctest-runner/MobileMcpSimulatorRunner.xcodeproj")
        );
        assert_eq!(
            relative_project("ios-real"),
            PathBuf::from("ios-real/xctest-runner/MobileMcpRealRunner.xcodeproj")
        );
    }
}

//! Release binaries are prod. Debug binaries are dev so both can run together.
//!
//! Prod keeps the installed layout: `~/.device-bot`, MCP `127.0.0.1:3337`, and
//! reports on `8765`. Dev uses `~/.device-bot-dev`, MCP `3338`, and reports on
//! `8766`.

use std::{io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Prod,
    Dev,
}

pub const FLAVOR: Flavor = if cfg!(debug_assertions) {
    Flavor::Dev
} else {
    Flavor::Prod
};

impl Flavor {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Prod => "prod",
            Self::Dev => "dev",
        }
    }

    pub const fn mcp_port(self) -> u16 {
        match self {
            Self::Prod => 3337,
            Self::Dev => 3338,
        }
    }

    pub const fn report_port(self) -> u16 {
        match self {
            Self::Prod => 8765,
            Self::Dev => 8766,
        }
    }

    pub const fn ios_agent_port(self) -> u16 {
        match self {
            Self::Prod => 18787,
            Self::Dev => 18797,
        }
    }

    pub const fn ios_sim_agent_port(self) -> u16 {
        match self {
            Self::Prod => 18788,
            Self::Dev => 18798,
        }
    }

    pub const fn home_dir_name(self) -> &'static str {
        match self {
            Self::Prod => ".device-bot",
            Self::Dev => ".device-bot-dev",
        }
    }

    pub fn mcp_address(self) -> String {
        format!("127.0.0.1:{}", self.mcp_port())
    }

    /// Bundle names this flavor may open, most specific first.
    ///
    /// Dev also accepts the unsuffixed Debug build so a local `flutter run`
    /// still resolves before the Xcode product rename is picked up.
    pub fn qc_bundle_names(self) -> &'static [&'static str] {
        if cfg!(target_os = "macos") {
            match self {
                Self::Prod => &["mobile_mcp_companion.app"],
                Self::Dev => &["mobile_mcp_companion_dev.app", "mobile_mcp_companion.app"],
            }
        } else if cfg!(windows) {
            match self {
                Self::Prod => &["mobile_mcp_companion.exe"],
                Self::Dev => &["mobile_mcp_companion_dev.exe", "mobile_mcp_companion.exe"],
            }
        } else {
            match self {
                Self::Prod => &["mobile_mcp_companion"],
                Self::Dev => &["mobile_mcp_companion_dev", "mobile_mcp_companion"],
            }
        }
    }

    pub fn qc_build_modes(self) -> &'static [&'static str] {
        match self {
            Self::Prod => &["Release", "Debug"],
            Self::Dev => &["Debug"],
        }
    }
}

pub fn home_dir() -> io::Result<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?;
    Ok(PathBuf::from(home).join(FLAVOR.home_dir_name()))
}

pub fn mcp_dir() -> io::Result<PathBuf> {
    Ok(home_dir()?.join("mcp"))
}

pub fn reports_dir() -> io::Result<PathBuf> {
    Ok(home_dir()?.join("reports"))
}

pub fn help() -> String {
    let flavor = FLAVOR;
    format!(
        "\
device-bot                         Start the background web app and open http://127.0.0.1:{report}
device-bot start                   Ensure the background web app is running
device-bot qc                      Open the QC desktop app and ensure local MCP is running
device-bot mcp                     Run the MCP server over stdio
device-bot mcp --listen HOST:PORT  Run the MCP server over HTTP
device-bot run <scenario.yaml> ... Run scenarios (see: device-bot run --help)
device-bot -v, --version
device-bot -h, --help
device-bot --configure-ios-signing  Detect the signing team and update the iOS helper project
device-bot --check-install
device-bot --check-update
device-bot -u, --upgrade            Install the latest verified release

This process is the {name} flavor.
Prod: ~/.device-bot, MCP 127.0.0.1:3337, reports http://127.0.0.1:8765.
Dev: ~/.device-bot-dev, MCP 127.0.0.1:3338, reports http://127.0.0.1:8766.
A debug build is dev. A release build, including the installed device-bot, is prod.
The web app survives closing the terminal. Repeated starts reuse the running service.
History override: DEVICE_BOT_REPORT_DATA_DIR. Report port override: DEVICE_BOT_REPORT_PORT.
",
        report = flavor.report_port(),
        name = flavor.name(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavors_keep_separate_ports_and_homes() {
        assert_eq!(Flavor::Prod.mcp_port(), 3337);
        assert_eq!(Flavor::Dev.mcp_port(), 3338);
        assert_eq!(Flavor::Prod.report_port(), 8765);
        assert_eq!(Flavor::Dev.report_port(), 8766);
        assert_eq!(Flavor::Prod.ios_agent_port(), 18787);
        assert_eq!(Flavor::Dev.ios_agent_port(), 18797);
        assert_eq!(Flavor::Prod.ios_sim_agent_port(), 18788);
        assert_eq!(Flavor::Dev.ios_sim_agent_port(), 18798);
        assert_eq!(Flavor::Prod.home_dir_name(), ".device-bot");
        assert_eq!(Flavor::Dev.home_dir_name(), ".device-bot-dev");
        assert!(
            Flavor::Dev
                .qc_bundle_names()
                .iter()
                .any(|name| name.contains("mobile_mcp_companion_dev"))
        );
        assert_eq!(Flavor::Dev.qc_build_modes(), ["Debug"]);
        assert_eq!(Flavor::Prod.qc_build_modes(), ["Release", "Debug"]);
        let help = help();
        assert!(help.contains("8765"), "{help}");
        assert!(help.contains("3338"), "{help}");
        assert!(help.contains(&format!("This process is the {} flavor", FLAVOR.name())));
    }

    #[test]
    fn home_paths_stay_inside_the_user_directory() {
        let home = home_dir().unwrap();
        assert!(home.ends_with(FLAVOR.home_dir_name()), "{}", home.display());
        assert_eq!(mcp_dir().unwrap(), home.join("mcp"));
        assert_eq!(reports_dir().unwrap(), home.join("reports"));
    }
}

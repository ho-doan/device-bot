//! Parse commands without reading configuration or starting services.
use std::{io, net::SocketAddr};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Launch { open_qc: bool },
    Qc,
    Run(Vec<String>),
    Help,
    McpHelp,
    Version,
    CheckInstall,
    Health(String),
    CheckUpdate,
    Upgrade,
    RequestScreenRecordingPermission,
    InstallChromeRecordingTools,
    IosDevelopmentTeam,
    ConfigureIosSigning,
    ProvisionIosXctest(String),
    Serve(Option<SocketAddr>),
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn no_more(arguments: &mut impl Iterator<Item = String>, message: &str) -> io::Result<()> {
    if arguments.next().is_some() {
        Err(invalid(message))
    } else {
        Ok(())
    }
}

pub(super) fn parse(arguments: impl IntoIterator<Item = String>) -> io::Result<Command> {
    let mut arguments = arguments.into_iter();
    let mut command = arguments.next();
    match command.as_deref() {
        None => return Ok(Command::Launch { open_qc: true }),
        Some("qc" | "start") => {
            let name = command.as_deref().unwrap();
            no_more(&mut arguments, &format!("{name} accepts no arguments"))?;
            return Ok(if name == "qc" {
                Command::Qc
            } else {
                Command::Launch { open_qc: false }
            });
        }
        Some("run") => return Ok(Command::Run(arguments.collect())),
        Some("--help" | "-h" | "help") => return Ok(Command::Help),
        Some("mcp") => {
            command = arguments.next();
            if matches!(command.as_deref(), Some("--help" | "-h")) {
                return Ok(Command::McpHelp);
            }
        }
        _ => {}
    }
    let value = match command.as_deref() {
        None => Command::Serve(None),
        Some("--version" | "-v") => {
            no_more(&mut arguments, "--version accepts no arguments")?;
            Command::Version
        }
        Some("--health") => {
            let message = "--health requires exactly one Simulator UDID or physical iPhone ID";
            let id = arguments
                .next()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid(message))?;
            no_more(&mut arguments, message)?;
            Command::Health(id)
        }
        Some("--provision-ios-xctest") => {
            let id = arguments
                .next()
                .ok_or_else(|| invalid("--provision-ios-xctest requires a paired iPhone UDID"))?;
            no_more(
                &mut arguments,
                "--provision-ios-xctest accepts exactly one device UDID",
            )?;
            Command::ProvisionIosXctest(id)
        }
        Some("--listen") => {
            let address = arguments
                .next()
                .ok_or_else(|| invalid("--listen requires HOST:PORT"))?;
            no_more(&mut arguments, "unexpected command-line argument")?;
            Command::Serve(Some(
                address
                    .parse()
                    .map_err(|e| invalid(format!("invalid listen address: {e}")))?,
            ))
        }
        Some(flag) => {
            let (value, message) = match flag {
                "--check-install" => (
                    Command::CheckInstall,
                    "--check-install accepts no arguments",
                ),
                "--check-update" => (Command::CheckUpdate, "--check-update accepts no arguments"),
                "--upgrade" | "-u" => (Command::Upgrade, "--upgrade accepts no arguments"),
                "--request-screen-recording-permission" => (
                    Command::RequestScreenRecordingPermission,
                    "--request-screen-recording-permission accepts no arguments",
                ),
                "--install-chrome-recording-tools" => (
                    Command::InstallChromeRecordingTools,
                    "--install-chrome-recording-tools accepts no arguments",
                ),
                "--ios-development-team" => (
                    Command::IosDevelopmentTeam,
                    "--ios-development-team accepts no arguments",
                ),
                "--configure-ios-signing" => (
                    Command::ConfigureIosSigning,
                    "--configure-ios-signing accepts no arguments; use DEVICE_BOT_IOS_TEAM to select a team",
                ),
                _ => {
                    return Err(invalid(format!(
                        "unknown argument: {flag}; see device-bot --help"
                    )));
                }
            };
            no_more(&mut arguments, message)?;
            value
        }
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_args(args: &[&str]) -> io::Result<Command> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn modes_and_forwarded_flow_arguments_stay_distinct() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Launch { open_qc: true });
        assert_eq!(
            parse_args(&["start"]).unwrap(),
            Command::Launch { open_qc: false }
        );
        assert_eq!(parse_args(&["mcp"]).unwrap(), Command::Serve(None));
        assert_eq!(
            parse_args(&["mcp", "--listen", "[::1]:3338"]).unwrap(),
            Command::Serve(Some("[::1]:3338".parse().unwrap()))
        );
        assert_eq!(
            parse_args(&["run", "--dry-run", "case.yaml"]).unwrap(),
            Command::Run(vec!["--dry-run".into(), "case.yaml".into()])
        );
        assert_eq!(parse_args(&["-v"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["mcp", "-u"]).unwrap(), Command::Upgrade);
    }

    #[test]
    fn invalid_arguments_fail_before_configuration_or_io() {
        for args in [
            vec!["start", "extra"],
            vec!["qc", "extra"],
            vec!["-u", "extra"],
            vec!["--health"],
            vec!["--health", " "],
            vec!["--health", "a", "b"],
            vec!["--listen", "bad"],
            vec!["mcp", "start"],
            vec!["--configure-ios-signing", "team"],
        ] {
            assert_eq!(
                parse_args(&args).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{args:?}"
            );
        }
    }
}

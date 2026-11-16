use std::{
    env,
    path::PathBuf,
    process::{Command, ExitCode},
};

use automation_test_harness::resolve_scenario;

fn usage() -> &'static str {
    "Usage: automation-test-harness --case NAME --suite DIR --mcp-url URL --profiles FILE --target TARGET --artifacts DIR [--runner PATH] [--include-draft]"
}

fn value(args: &[String], flag: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|argument| argument == flag)
        .ok_or_else(|| format!("missing {flag}\n{}", usage()))?;
    args.get(index + 1)
        .filter(|next| !next.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value\n{}", usage()))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("automation-test-harness: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let include_draft = args.iter().any(|argument| argument == "--include-draft");
    let suite = PathBuf::from(value(&args, "--suite")?);
    let name = value(&args, "--case")?;
    let scenario = resolve_scenario(&suite, &name, include_draft).map_err(|e| e.to_string())?;
    let artifacts = PathBuf::from(value(&args, "--artifacts")?);
    let profiles = PathBuf::from(value(&args, "--profiles")?);
    let mcp_url = value(&args, "--mcp-url")?;
    let target = value(&args, "--target")?;
    let runner = args
        .iter()
        .position(|argument| argument == "--runner")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|dir| dir.join("robot-flow")))
                .unwrap_or_else(|| PathBuf::from("robot-flow"))
        });

    let status = Command::new(runner)
        .arg("--mcp-url")
        .arg(mcp_url)
        .arg("--profiles")
        .arg(profiles)
        .arg("--target")
        .arg(target)
        .arg("--artifacts")
        .arg(artifacts)
        .arg(scenario)
        .status()
        .map_err(|error| format!("could not launch robot-flow: {error}"))?;

    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8)
    })
}

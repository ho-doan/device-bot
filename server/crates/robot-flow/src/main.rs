use robot_flow::{FlowDocument, FlowReport, FlowRunner, Profile, Profiles, mcp::StdioMcp, schema};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default)]
struct Options {
    dry: bool,
    profiles: Option<PathBuf>,
    target: Option<String>,
    device: Option<String>,
    app: Option<String>,
    server: String,
    mcp_url: Option<String>,
    server_args: Vec<String>,
    artifacts: PathBuf,
    confirmed: Vec<String>,
    rpc: u64,
    case: u64,
    paths: Vec<String>,
}

fn parse() -> Result<Options, String> {
    let mut o = Options {
        server: env::var("DEVICE_BOT_BIN").unwrap_or("device-bot".into()),
        artifacts: "artifacts/flows".into(),
        rpc: 180,
        case: 600,
        ..Options::default()
    };
    let mut args = env::args().skip(1);
    let mut custom_server = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "device-bot run [--dry-run] [--profiles targets.yaml --target NAME] [--device ID] [--app-id ID]\n  [--server PROGRAM --server-arg ARG | --mcp-url http://127.0.0.1:3337/mcp] [--artifacts DIR] [--confirm PRECONDITION]\n  [--rpc-timeout SECONDS] [--case-timeout SECONDS] <scenario.yaml> ...\n\nDefault MCP command: device-bot mcp. Also available directly as robot-flow."
                );
                std::process::exit(0);
            }
            "--version" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--mcp-url" => o.mcp_url = Some(args.next().ok_or("--mcp-url needs a URL")?),
            "--dry-run" => o.dry = true,
            "--profiles" => o.profiles = Some(args.next().ok_or("--profiles needs a path")?.into()),
            "--target" => o.target = Some(args.next().ok_or("--target needs a name")?),
            "--device" => o.device = Some(args.next().ok_or("--device needs an ID")?),
            "--app-id" => o.app = Some(args.next().ok_or("--app-id needs an ID")?),
            "--server" => {
                o.server = args.next().ok_or("--server needs a program")?;
                custom_server = true;
            }
            "--server-arg" => o
                .server_args
                .push(args.next().ok_or("--server-arg needs an argument")?),
            "--artifacts" => o.artifacts = args.next().ok_or("--artifacts needs a path")?.into(),
            "--confirm" => o
                .confirmed
                .push(args.next().ok_or("--confirm needs a precondition")?),
            "--rpc-timeout" => {
                o.rpc = args
                    .next()
                    .ok_or("--rpc-timeout needs seconds")?
                    .parse()
                    .map_err(|_| "invalid RPC timeout")?
            }
            "--case-timeout" => {
                o.case = args
                    .next()
                    .ok_or("--case-timeout needs seconds")?
                    .parse()
                    .map_err(|_| "invalid case timeout")?
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            _ => o.paths.push(arg),
        }
    }
    if o.paths.is_empty() {
        return Err("provide at least one scenario YAML (see --help)".into());
    }
    if o.mcp_url.is_some() && custom_server {
        return Err("choose --mcp-url or --server, not both".into());
    }
    if !custom_server {
        o.server_args.insert(0, "mcp".into());
    }
    if o.profiles.is_some() != o.target.is_some() {
        return Err("--profiles and --target are required together".into());
    }
    if o.rpc == 0 || o.rpc > 600 || o.case == 0 || o.case > 3600 {
        return Err("timeouts must be positive; RPC <=600s, case <=3600s".into());
    }
    Ok(o)
}

fn profile(o: &Options) -> Result<Profile, String> {
    let mut p = if let (Some(path), Some(target)) = (&o.profiles, &o.target) {
        Profiles::from_path(path)?
            .targets
            .remove(target)
            .ok_or_else(|| format!("unknown target {target}"))?
    } else {
        Profile::default()
    };
    if let Some(id) = &o.device {
        if !p.device.is_object() {
            p.device = json!({});
        }
        p.device["id"] = json!(id);
    }
    if let Some(app) = &o.app {
        p.app_id = app.clone();
    }
    Ok(p)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn save_report(report: &FlowReport, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("report.json"),
        serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let failure = if let Some(error) = &report.error {
        format!(
            "<{} message=\"{}\"/>",
            if report.error_kind.as_deref() == Some("assertion") {
                "failure"
            } else {
                "error"
            },
            escape(error)
        )
    } else if report.status == "PARTIAL" || report.dry_run {
        format!(
            "<skipped message=\"{}\"/>",
            escape(&if report.dry_run {
                "dry-run only; no device coverage".into()
            } else {
                report.limitations.join("; ")
            })
        )
    } else {
        String::new()
    };
    let junit = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><testsuite name=\"robot-flow\" tests=\"1\" errors=\"{}\" failures=\"{}\" skipped=\"{}\"><testcase name=\"{}\">{failure}</testcase></testsuite>",
        usize::from(report.error.is_some() && report.error_kind.as_deref() != Some("assertion")),
        usize::from(report.error_kind.as_deref() == Some("assertion")),
        usize::from(report.status == "PARTIAL" || report.dry_run),
        escape(&report.name)
    );
    fs::write(dir.join("junit.xml"), junit).map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() {
    let o = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2)
        }
    };
    let mut failed = false;
    for (i, path) in o.paths.iter().enumerate() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = o
            .artifacts
            .join(format!("case-{}-{}-{stamp}", i + 1, std::process::id()));
        let prepared = (|| {
            let flow = FlowDocument::from_path(path)?;
            let profile = profile(&o)?;
            if !o.dry
                && o.paths.len() > 1
                && (!profile.preconditions.is_empty() || !flow.preconditions.is_empty())
            {
                return Err(
                    "run one case at a time and repeat external fixture setup before each case"
                        .into(),
                );
            }
            schema::compile(&flow, &profile)?;
            if !o.dry {
                for p in profile.preconditions.iter().chain(&flow.preconditions) {
                    if !o.confirmed.contains(p) {
                        return Err(format!("setup precondition not confirmed: {p}"));
                    }
                }
            }
            Ok((flow, profile))
        })();
        let report = match prepared {
            Ok((flow, profile)) => {
                let client = if o.dry {
                    Ok(None)
                } else if let Some(url) = &o.mcp_url {
                    robot_flow::http_mcp::HttpMcp::new(url, Duration::from_secs(o.rpc))
                        .map(|c| Some(Box::new(c) as Box<dyn robot_flow::mcp::Mcp>))
                } else {
                    StdioMcp::spawn(&o.server, &o.server_args, Duration::from_secs(o.rpc))
                        .map(|c| Some(Box::new(c) as Box<dyn robot_flow::mcp::Mcp>))
                };
                match client {
                    Ok(client) => {
                        let mut runner = FlowRunner::new(client, dir.clone());
                        let mut report = runner
                            .run(&flow, &profile, &o.confirmed, Duration::from_secs(o.case))
                            .await;
                        if !o.dry {
                            if let Err(error) = runner.annotate_video(&report).await {
                                report.status = "FAIL".into();
                                if let Some(existing) = report.error.as_mut() {
                                    existing.push_str(&format!("; {error}"));
                                } else {
                                    report.error = Some(error);
                                    report.error_kind = Some("artifact".into());
                                }
                            }
                        }
                        report
                    }
                    Err(error) => setup_error(&flow.name, o.dry, error),
                }
            }
            Err(error) => setup_error(&format!("scenario-{}", i + 1), o.dry, error),
        };
        if !o.dry {
            if let Err(e) = save_report(&report, &dir) {
                eprintln!("cannot save report: {e}");
                failed = true;
            } else {
                eprintln!("Evidence: {}", dir.display());
            }
        }
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        failed |= report.error.is_some();
        if failed {
            break;
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn setup_error(name: &str, dry_run: bool, error: String) -> FlowReport {
    FlowReport {
        name: name.into(),
        dry_run,
        status: "FAIL".into(),
        selected_device: None,
        steps: Vec::new(),
        limitations: Vec::new(),
        preconditions: Vec::new(),
        error: Some(error),
        error_kind: Some("setup".into()),
    }
}

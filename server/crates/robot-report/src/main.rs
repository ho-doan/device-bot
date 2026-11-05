use robot_report::Store;
use std::{collections::BTreeMap, env, io, net::SocketAddr, path::PathBuf};

const HELP: &str = "robot-report start [--listen 127.0.0.1:8765] [--data-dir artifacts/qa-history]\nrobot-report project-add --name NAME --source DIR [--data-dir DIR]\nrobot-report publish --project ID --input DIR --name RUN [--data-dir DIR]\nrobot-report projects [--data-dir DIR]\n\nstart runs continuously until Ctrl-C. publish snapshots a completed run and preserves every previous run.\nUse --listen 0.0.0.0:8765 for QA read/download access over LAN; manage projects at localhost or through this CLI.";

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "--help".into());
    if ["--help", "-h"].contains(&command.as_str()) {
        println!("{HELP}");
        return Ok(());
    }
    let mut options = BTreeMap::new();
    while let Some(flag) = args.next() {
        if !flag.starts_with("--") || options.contains_key(&flag) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid argument {flag}"),
            ));
        }
        let value = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Missing value for {flag}"),
            )
        })?;
        options.insert(flag, value);
    }
    let allowed: &[&str] = match command.as_str() {
        "start" => &["--listen", "--data-dir"],
        "project-add" => &["--name", "--source", "--data-dir"],
        "publish" => &["--project", "--input", "--name", "--data-dir"],
        "projects" => &["--data-dir"],
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, HELP)),
    };
    if let Some(key) = options.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unknown option {key}"),
        ));
    }
    let required = |key: &str| {
        options
            .get(key)
            .filter(|s| !s.trim().is_empty())
            .map(String::as_str)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("Required: {key}")))
    };
    let data = PathBuf::from(
        options
            .get("--data-dir")
            .map(String::as_str)
            .unwrap_or("artifacts/qa-history"),
    );
    let store = Store::open(data)?;
    match command.as_str() {
        "project-add" => println!(
            "{}",
            serde_json::to_string_pretty(
                &store
                    .create_project(required("--name")?, &PathBuf::from(required("--source")?))?
            )?
        ),
        "publish" => println!(
            "{}",
            serde_json::to_string_pretty(&store.import_run(
                required("--project")?,
                required("--name")?,
                &PathBuf::from(required("--input")?)
            )?)?
        ),
        "projects" => println!("{}", serde_json::to_string_pretty(&store.projects()?)?),
        "start" => {
            let address: SocketAddr = options
                .get("--listen")
                .map(String::as_str)
                .unwrap_or("127.0.0.1:8765")
                .parse()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            println!(
                "QA reports: http://{}\nHistory: {}",
                listener.local_addr()?,
                store.root().display()
            );
            axum::serve(
                listener,
                robot_report::router(store).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown())
            .await?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = terminate.recv() => {},
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

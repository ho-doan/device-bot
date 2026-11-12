//! Open the optional, prebuilt QC desktop companion without requiring Flutter.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn app() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEVICE_BOT_QC_APP") {
        let path = PathBuf::from(path);
        return path.exists().then_some(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "DEVICE_BOT_QC_APP does not exist")
        });
    }
    let flavor = super::flavor::FLAVOR;
    let mut candidates = Vec::new();
    let executable_dir = std::env::current_exe()?;
    for name in flavor.qc_bundle_names() {
        candidates.push(executable_dir.with_file_name(name));
    }
    if flavor == super::flavor::Flavor::Prod && cfg!(target_os = "macos") {
        let name = flavor.qc_bundle_names()[0];
        candidates.push(PathBuf::from("/Applications").join(name));
        candidates
            .push(PathBuf::from("/Applications/Device Bot QC.app/Contents/Resources").join(name));
        if let Some(home) = std::env::var_os("HOME") {
            let applications = PathBuf::from(home).join("Applications");
            candidates.push(applications.join(name));
            candidates.push(
                applications
                    .join("Device Bot QC.app/Contents/Resources")
                    .join(name),
            );
        }
    }
    if let Some(source) = super::flutter_app_directory() {
        for mode in flavor.qc_build_modes() {
            for name in flavor.qc_bundle_names() {
                let relative = if cfg!(target_os = "macos") {
                    format!("build/macos/Build/Products/{mode}/{name}")
                } else if cfg!(windows) {
                    format!("build/windows/x64/runner/{mode}/{name}")
                } else {
                    format!("build/linux/x64/{}/bundle/{name}", mode.to_lowercase())
                };
                candidates.push(source.join(relative));
            }
        }
    }
    candidates.into_iter().find(|path| path.exists()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            if flavor == super::flavor::Flavor::Dev {
                "QC dev app not found. From server/flutter_app run `flutter run -d macos` (Windows: `flutter run -d windows`). The installed device-bot stays on prod."
            } else {
                "QC app not found. The CLI bundle does not include QC. Build server/flutter_app with `flutter build macos` (Windows: `flutter build windows`), or set DEVICE_BOT_QC_APP to an existing app bundle/executable."
            },
        )
    })
}

fn ready(address: SocketAddr) -> io::Result<bool> {
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "GET /health/live HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.take(8192).read_to_string(&mut response)?;
    if response
        .split_once("\r\n\r\n")
        .is_some_and(|(headers, body)| {
            headers
                .lines()
                .next()
                .is_some_and(|line| line.split_whitespace().nth(1) == Some("200"))
                && serde_json::from_str::<serde_json::Value>(body)
                    .is_ok_and(|body| body["service"] == "device-bot" && body["status"] == "ok")
        })
    {
        return Ok(true);
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!(
            "{address} is occupied by another service or an older Device Bot. Stop it before running device-bot qc."
        ),
    ))
}

fn ensure_server() -> io::Result<()> {
    let address = super::flavor::FLAVOR.mcp_address().parse().unwrap();
    if ready(address)? {
        return Ok(());
    }
    let directory = super::flavor::mcp_dir()?;
    fs::create_dir_all(&directory)?;
    let log_path = directory.join("server.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let listen = super::flavor::FLAVOR.mcp_address();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("mcp")
        .arg("--listen")
        .arg(&listen)
        .env("DEVICE_BOT_FLUTTER_AUTOSTART", "0")
        .current_dir(&directory)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    super::launcher::detach(&mut command);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if ready(address).unwrap_or(false) {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "QC MCP server exited with {status}. See {}",
                log_path.display()
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "QC MCP server did not become ready. See {}",
                    log_path.display()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn launch(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|extension| extension == "app") {
        let mut command = Command::new("open");
        command.arg(path);
        if std::env::var_os("HOME").is_some() {
            command.env("DEVICE_BOT_QC_CONNECTION_DIR", super::flavor::mcp_dir()?);
        }
        let status = command.status()?;
        return if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("Could not open QC: {status}")))
        };
    }
    let mut command = Command::new(path);
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    command.stdout(Stdio::null()).stderr(Stdio::null());
    super::launcher::detach(&mut command);
    command.spawn()?;
    Ok(())
}

pub fn open() -> io::Result<()> {
    // Resolve first: missing QC must not leave a server running unnecessarily.
    let app = fs::canonicalize(app()?)?;
    ensure_server()?;
    launch(&app)?;
    println!(
        "QC {}: {} (MCP http://{})",
        super::flavor::FLAVOR.name(),
        app.display(),
        super::flavor::FLAVOR.mcp_address()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn readiness_reuses_only_device_bot_without_device_discovery() {
        for (body, valid) in [
            (r#"{"service":"device-bot","status":"ok"}"#, true),
            (r#"{"service":"robot-report","status":"ok"}"#, false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 1024];
                let count = stream.read(&mut request).unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..count]).starts_with("GET /health/live ")
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let result = ready(address);
            server.join().unwrap();
            if valid {
                assert!(result.unwrap());
            } else {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AddrInUse);
            }
        }
    }

    #[test]
    fn closed_port_needs_server_start() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert!(!ready(address).unwrap());
    }
}

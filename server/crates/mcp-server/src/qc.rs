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
    if let Some(directory) = std::env::current_exe()?.parent() {
        for name in flavor.qc_bundle_names() {
            // Release installers copy the QC bundle into this directory.
            candidates.push(directory.join("qc").join(name));
            candidates.push(directory.join(name));
        }
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
                "QC app not found. The release installer places it in the qc directory next to device-bot. Build server/flutter_app with `flutter build macos` (Windows: `flutter build windows`), or set DEVICE_BOT_QC_APP to an existing app bundle/executable."
            },
        )
    })
}

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq)]
enum LiveState {
    Down,
    Current,
    /// Device Bot is listening, but it is older than this executable or omits its version.
    Stale,
}

fn live_state(address: SocketAddr) -> io::Result<LiveState> {
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(LiveState::Down);
        }
        Err(error) => return Err(error),
    };
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "GET /health/live HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;
    let response = read_http_response(&mut stream)?;
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return Err(occupied(address));
    };
    if headers
        .lines()
        .next()
        .is_none_or(|line| line.split_whitespace().nth(1) != Some("200"))
    {
        return Err(occupied(address));
    }
    let Ok(body) = serde_json::from_str::<serde_json::Value>(body) else {
        return Err(occupied(address));
    };
    if body["service"] != "device-bot" || body["status"] != "ok" {
        return Err(occupied(address));
    }
    if body["version"].as_str() == Some(SERVER_VERSION) {
        Ok(LiveState::Current)
    } else {
        Ok(LiveState::Stale)
    }
}

fn read_http_response(stream: &mut TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        if bytes.len() >= 8192 {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // A peer can deliver the response and then abort the socket.
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset && !bytes.is_empty() => {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn occupied(address: SocketAddr) -> io::Error {
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("{address} is occupied by another service. Stop it before running device-bot qc."),
    )
}

fn connection_pid(directory: &Path, port: u16) -> Option<u32> {
    let path = directory.join(format!("connection-{port}.json"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    u32::try_from(value.get("pid")?.as_u64()?).ok()
}

fn command_is_device_bot(command: &str) -> bool {
    command.split_whitespace().any(|token| {
        let name = token
            .trim_matches('"')
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(token);
        let name = name.trim_matches('"').to_ascii_lowercase();
        name == "device-bot" || name == "device-bot.exe"
    })
}

fn process_command(pid: u32) -> io::Result<String> {
    let output = if cfg!(windows) {
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine"),
            ])
            .output()?
    } else {
        Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()?
    };
    if !output.status.success() {
        return Err(io::Error::other(format!("could not inspect process {pid}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn signal_process(pid: u32, force: bool) -> io::Result<()> {
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()?
    } else if force {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()?
    } else {
        Command::new("kill").arg(pid.to_string()).status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "could not stop process {pid}: {status}"
        )))
    }
}

fn stop_stale_server(address: SocketAddr) -> io::Result<()> {
    let directory = super::flavor::mcp_dir()?;
    let pid = connection_pid(&directory, address.port()).ok_or_else(|| {
        io::Error::other(format!(
            "{address} is an older Device Bot. Stop it, then run device-bot qc again."
        ))
    })?;
    let command = process_command(pid).unwrap_or_default();
    if !command_is_device_bot(&command) {
        return Err(io::Error::other(format!(
            "{address} is owned by process {pid}, which is not device-bot. Stop it before running device-bot qc."
        )));
    }
    eprintln!(
        "Stopping Device Bot on {address} (pid {pid}) so QC can use version {SERVER_VERSION}."
    );
    signal_process(pid, false)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(live_state(address), Ok(LiveState::Down)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    signal_process(pid, true)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if matches!(live_state(address), Ok(LiveState::Down)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{address} did not stop after replacing the older Device Bot"),
    ))
}

fn ensure_server() -> io::Result<()> {
    let address = super::flavor::FLAVOR
        .mcp_loopback_address()
        .parse()
        .unwrap();
    match live_state(address)? {
        LiveState::Current => return Ok(()),
        LiveState::Down => {}
        LiveState::Stale => stop_stale_server(address)?,
    }
    let directory = super::flavor::mcp_dir()?;
    fs::create_dir_all(&directory)?;
    let log_path = directory.join("server.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let listen = super::flavor::FLAVOR.mcp_listen_address();
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
        if matches!(live_state(address), Ok(LiveState::Current)) {
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
        "QC {}: {} (MCP http://{} ; LAN QC /qc/ and reports /reports/)",
        super::flavor::FLAVOR.name(),
        app.display(),
        super::flavor::FLAVOR.mcp_loopback_address()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn probe(body: &str) -> io::Result<LiveState> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let count = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET /health/live "));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            // live_state may close after the body; BrokenPipe is the expected peer.
            let _ = write!(stream, "{response}");
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });
        let result = live_state(address);
        server.join().unwrap();
        result
    }

    #[test]
    fn readiness_reuses_only_the_current_device_bot() {
        let current = format!(
            r#"{{"service":"device-bot","status":"ok","version":"{}"}}"#,
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(probe(&current).unwrap(), LiveState::Current);
        assert_eq!(
            probe(r#"{"service":"device-bot","status":"ok"}"#).unwrap(),
            LiveState::Stale
        );
        assert_eq!(
            probe(r#"{"service":"robot-report","status":"ok"}"#)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AddrInUse
        );
    }

    #[test]
    fn closed_port_needs_server_start() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert_eq!(live_state(address).unwrap(), LiveState::Down);
    }

    #[test]
    fn only_the_device_bot_executable_can_be_replaced() {
        assert!(command_is_device_bot(
            "/usr/local/bin/device-bot mcp --listen 127.0.0.1:3337"
        ));
        assert!(command_is_device_bot(
            r#""C:\Users\qc\.device-bot\bin\device-bot.exe" mcp --listen 127.0.0.1:3337"#
        ));
        assert!(!command_is_device_bot(
            "/Applications/SomeApp.app/Contents/MacOS/SomeApp"
        ));
        assert!(!command_is_device_bot(""));
    }

    #[test]
    fn connection_file_records_the_server_pid() {
        let root = std::env::temp_dir().join(format!("qc-pid-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("connection-3337.json"),
            r#"{"pid":4242,"url":"http://127.0.0.1:3337"}"#,
        )
        .unwrap();
        assert_eq!(connection_pid(&root, 3337), Some(4242));
        assert_eq!(connection_pid(&root, 3338), None);
        fs::remove_dir_all(root).unwrap();
    }
}

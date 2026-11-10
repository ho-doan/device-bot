//! User-facing entry points for the installed native CLI suite.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const HELP: &str =
    "device-bot                         Start the background web app and open http://127.0.0.1:8765
device-bot start                   Ensure the background web app is running
device-bot mcp                     Run the MCP server over stdio
device-bot mcp --listen HOST:PORT  Run the MCP server over HTTP
device-bot run <scenario.yaml> ... Run scenarios (see: device-bot run --help)
device-bot --version
device-bot --configure-ios-signing  Detect the signing team and update the iOS helper project
device-bot --check-install
device-bot --check-update
device-bot --upgrade

The web app survives closing the terminal. Repeated starts reuse the running service.
History defaults to ~/.device-bot/reports; override with DEVICE_BOT_REPORT_DATA_DIR.
The default web port is 8765; override with DEVICE_BOT_REPORT_PORT.";

fn sibling(name: &str) -> io::Result<PathBuf> {
    let path =
        std::env::current_exe()?.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Missing {}. Reinstall the complete Device Bot bundle.",
                path.display()
            ),
        ));
    }
    Ok(path)
}

pub fn run(arguments: impl Iterator<Item = String>) -> io::Result<()> {
    let mut command = Command::new(sibling("robot-flow")?);
    command.args(arguments);
    if std::env::var_os("DEVICE_BOT_BIN").is_none() {
        command.env("DEVICE_BOT_BIN", std::env::current_exe()?);
    }
    let status = command.status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn report_directory() -> io::Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("DEVICE_BOT_REPORT_DATA_DIR") {
        PathBuf::from(path)
    } else {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?;
        PathBuf::from(home).join(".device-bot/reports")
    };
    fs::create_dir_all(&path)?;
    fs::canonicalize(path)
}

// A TCP listener alone is insufficient: another application might own port 8765.
fn report_ready(address: SocketAddr) -> io::Result<bool> {
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
        Ok(stream) => stream,
        // Windows can retry a refused loopback connection beyond this deadline.
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
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    write!(
        stream,
        "GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.take(8192).read_to_string(&mut response)?;
    let valid = response
        .split_once("\r\n\r\n")
        .is_some_and(|(headers, body)| {
            headers
                .lines()
                .next()
                .is_some_and(|line| matches!(line.split_whitespace().nth(1), Some("200")))
                && serde_json::from_str::<serde_json::Value>(body)
                    .is_ok_and(|body| body["status"] == "ok" && body["service"] == "robot-report")
        });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("{address} is occupied by another service; Device Bot was not started"),
        ));
    }
    Ok(true)
}

fn detach(command: &mut Command) {
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Detach from the console so closing PowerShell cannot stop the web app.
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only the async-signal-safe setsid syscall runs between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

pub fn start(open: bool) -> io::Result<()> {
    let port = match std::env::var("DEVICE_BOT_REPORT_PORT") {
        Ok(value) => value
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "DEVICE_BOT_REPORT_PORT must be 1..65535",
                )
            })?,
        Err(std::env::VarError::NotPresent) => 8765,
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    };
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let url = format!("http://{address}");
    if !report_ready(address)? {
        let binary = sibling("robot-report")?;
        let data = report_directory()?;
        let log_path = data.join("server.log");
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let mut command = Command::new(binary);
        command
            .args(["start", "--listen", &address.to_string(), "--data-dir"])
            .arg(&data)
            .current_dir(&data)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        detach(&mut command);
        let mut child = command.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if report_ready(address).unwrap_or(false) {
                break;
            }
            if let Some(status) = child.try_wait()? {
                // Another simultaneous start may have won the port race.
                if report_ready(address).unwrap_or(false) {
                    break;
                }
                return Err(io::Error::other(format!(
                    "Web server exited with {status}. See {}",
                    log_path.display()
                )));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "Web server did not become ready. See {}",
                        log_path.display()
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Dropping Child leaves the detached service running.
    }
    println!("Device Bot web: {url}");
    if open {
        open_browser(&url)?;
    }
    Ok(())
}

fn open_browser(url: &str) -> io::Result<()> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command.status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "Could not open the browser. Open {url} manually."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn probe(response: &'static str) -> io::Result<bool> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            // A TCP read can return only part of write!'s HTTP request. Drain
            // its headers before closing, or unread bytes can trigger a reset.
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                connection.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            connection.write_all(response.as_bytes()).unwrap();
        });
        let result = report_ready(address);
        server.join().unwrap();
        result
    }

    #[test]
    fn a_closed_port_means_the_service_needs_starting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert!(!report_ready(address).unwrap());
    }

    #[test]
    fn identifies_the_report_service_before_reusing_a_listener() {
        assert!(probe("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"service\":\"robot-report\"}").unwrap());
        for response in [
            "HTTP/1.1 200 OK\r\n\r\n{\"status\":\"ok\",\"service\":\"other\"}",
            "HTTP/1.1 503 Unavailable\r\n\r\n{\"status\":\"ok\",\"service\":\"robot-report\"}",
            "HTTP/1.1 200 OK\r\n\r\n<html>Other app</html>",
        ] {
            assert_eq!(
                probe(response).unwrap_err().kind(),
                io::ErrorKind::AddrInUse
            );
        }
    }
}

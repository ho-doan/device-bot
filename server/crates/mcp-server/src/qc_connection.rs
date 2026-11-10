//! Same-user connection discovery for the local QC desktop app.
use std::{
    fs, io,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub fn publish(address: SocketAddr, token: &str) -> io::Result<()> {
    if !address.ip().is_loopback() && !address.ip().is_unspecified() {
        return Ok(());
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| io::Error::other("user home directory is unavailable"))?;
    let home = PathBuf::from(home);
    write_connection(&home.join(".device-bot/mcp"), address, token)?;
    #[cfg(target_os = "macos")]
    {
        // QC keeps App Sandbox enabled and reads its own private container.
        let container = home.join("Library/Containers/com.example.mobileMcpCompanion/Data");
        if container.is_dir() {
            write_connection(&container.join(".device-bot/mcp"), address, token)?;
        }
    }
    Ok(())
}

fn write_connection(directory: &Path, address: SocketAddr, token: &str) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let path = directory.join(format!("connection-{}.json", address.port()));
    let temporary = directory.join(format!(".connection-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        let host = if address.ip().is_unspecified() {
            "127.0.0.1".to_owned()
        } else if address.is_ipv6() {
            format!("[{}]", address.ip())
        } else {
            address.ip().to_string()
        };
        let value = serde_json::json!({"url":format!("http://{host}:{}", address.port()),
            "bearerToken":token, "pid":std::process::id()});
        file.write_all(serde_json::to_string(&value)?.as_bytes())?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::rename(&temporary, &path)
    })();
    let _ = fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_discovery_is_private_and_updated_on_restart() {
        let root = std::env::temp_dir().join(format!("qc-connection-{}", uuid::Uuid::new_v4()));
        let address = "127.0.0.1:3337".parse().unwrap();
        write_connection(&root, address, "first-token").unwrap();
        write_connection(&root, address, "replacement-token").unwrap();
        let path = root.join("connection-3337.json");
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["url"], "http://127.0.0.1:3337");
        assert_eq!(value["bearerToken"], "replacement-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

//! A loopback-only static server owned by one project run.
use crate::invalid;
use axum::Router;
use serde::Deserialize;
use std::{io, path::Path};
use tower_http::services::ServeDir;

pub(crate) const DRY_RUN_URL: &str = "http://127.0.0.1:1/__device_bot_web_dry_run__";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebConfig {
    pub(crate) base_url_env: String,
}

pub(crate) struct WebServer {
    pub(crate) url: String,
    task: tokio::task::JoinHandle<()>,
}

impl WebServer {
    pub(crate) async fn start(root: &Path) -> io::Result<Self> {
        let index = root.join("index.html");
        let metadata =
            std::fs::metadata(&index).map_err(|_| invalid("web artifact is missing index.html"))?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(invalid("web artifact index.html is invalid or too large"));
        }
        let html = std::fs::read_to_string(index)
            .map_err(|_| invalid("web artifact index.html is not UTF-8"))?;
        let base = base_path(&html)?;
        let service = ServeDir::new(root);
        let app = if base == "/" {
            Router::new().fallback_service(service)
        } else {
            Router::new().nest_service(base.trim_end_matches('/'), service)
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            url: format!("http://127.0.0.1:{port}{}", base.trim_end_matches('/')),
            task,
        })
    }
}

impl Drop for WebServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn base_path(html: &str) -> io::Result<String> {
    let element = regex::Regex::new(r"(?is)<base\b[^>]*>").expect("constant regex");
    let attribute =
        regex::Regex::new(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("constant regex");
    let base = match element.find(html) {
        None => "/",
        Some(tag) => {
            let captures = attribute
                .captures(tag.as_str())
                .ok_or_else(|| invalid("web artifact base href must be quoted"))?;
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .unwrap()
                .as_str()
        }
    };
    if base.len() > 1024
        || !base.starts_with('/')
        || !base.ends_with('/')
        || base.contains("//")
        || base.split('/').any(|part| part == "." || part == "..")
        || !base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.~".contains(&b))
    {
        return Err(invalid(
            "web artifact base href must be an absolute local path ending in /",
        ));
    }
    Ok(base.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn get(port: u16, path: &str) -> String {
        let mut socket = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        socket
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn serves_a_local_base_path_and_stops_when_dropped() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("web");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "<base href='/app/'>fixture").unwrap();
        std::fs::write(root.join("main.js"), "javascript fixture").unwrap();
        std::fs::write(temp.path().join("private.txt"), "private fixture").unwrap();
        let server = WebServer::start(&root).await.unwrap();
        let parsed = url::Url::parse(&server.url).unwrap();
        let port = parsed.port().unwrap();
        assert_eq!(parsed.path(), "/app");
        assert!(get(port, "/app/").await.contains("fixture"));
        assert!(
            get(port, "/app/main.js")
                .await
                .contains("javascript fixture")
        );
        assert!(get(port, "/main.js").await.starts_with("HTTP/1.1 404"));
        assert!(
            !get(port, "/app/%2e%2e/private.txt")
                .await
                .contains("private fixture")
        );
        drop(server);
        let closed = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(closed.is_ok());
    }

    #[test]
    fn rejects_external_or_unsafe_base_paths() {
        for value in [
            "https://example.test/",
            "//other/",
            "/a/../",
            "/foo?x/",
            "/foo",
        ] {
            assert!(
                base_path(&format!("<base href=\"{value}\">")).is_err(),
                "{value}"
            );
        }
        assert_eq!(base_path("<html></html>").unwrap(), "/");
    }
}

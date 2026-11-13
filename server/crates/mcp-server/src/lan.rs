//! LAN browser access: CORS, QC static files, and nested robot-report.
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};

pub(crate) async fn cors(request: Request, next: Next) -> Response {
    let origin = request.headers().get(header::ORIGIN).cloned();
    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_cors(response.headers_mut(), origin.as_ref());
        return response;
    }
    let mut response = next.run(request).await;
    apply_cors(response.headers_mut(), origin.as_ref());
    response
}

fn apply_cors(headers: &mut HeaderMap, origin: Option<&HeaderValue>) {
    let Some(origin) = origin.and_then(|value| value.to_str().ok()) else {
        return;
    };
    if !lan_origin_allowed(origin) {
        return;
    }
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type, mcp-session-id, Accept"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("mcp-session-id"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
}

pub(crate) fn lan_origin_allowed(origin: &str) -> bool {
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return false;
    }
    let Some(host) = uri.host() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(address) => address.is_loopback() || address.is_private(),
        IpAddr::V6(address) => address.is_loopback() || address.is_unique_local(),
    }
}

pub fn qc_web_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEVICE_BOT_QC_WEB_DIR") {
        let path = PathBuf::from(path);
        return path.join("index.html").is_file().then_some(path);
    }
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("qc-web"));
            candidates.push(directory.join("qc").join("web"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../flutter_app/build/web"));
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
}

#[derive(Clone)]
struct QcRoot(PathBuf);

pub(crate) fn qc_router(root: PathBuf) -> Router {
    Router::new()
        .route("/qc", get(serve_qc_index))
        .route("/qc/", get(serve_qc_index))
        .route("/qc/{*path}", get(serve_qc_asset))
        .with_state(QcRoot(root))
}

async fn serve_qc_index(State(QcRoot(root)): State<QcRoot>) -> Response {
    serve_qc(&root, "/").await
}

async fn serve_qc_asset(State(QcRoot(root)): State<QcRoot>, uri: Uri) -> Response {
    serve_qc(&root, uri.path()).await
}

async fn serve_qc(root: &Path, request_path: &str) -> Response {
    let relative = request_path
        .strip_prefix("/qc/")
        .or_else(|| request_path.strip_prefix("/qc"))
        .unwrap_or(request_path)
        .trim_start_matches('/');
    let file = if relative.is_empty() {
        root.join("index.html")
    } else {
        match contained(root, relative) {
            Some(path) if path.is_file() => path,
            _ => root.join("index.html"),
        }
    };
    let bytes = match tokio::fs::read(&file).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let mut body = bytes;
    if file.file_name().is_some_and(|name| name == "index.html") {
        let html =
            String::from_utf8_lossy(&body).replace("<base href=\"/\">", "<base href=\"/qc/\">");
        let html = html.replace("<base href=\"$FLUTTER_BASE_HREF\">", "<base href=\"/qc/\">");
        body = html.into_bytes();
    }
    let content_type = content_type(&file);
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(content_type))],
        Body::from(body),
    )
        .into_response()
}

fn contained(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

pub fn advertise(address: SocketAddr, lan_hosts: &[String]) -> Vec<String> {
    let port = address.port();
    let mut urls = vec![format!("http://127.0.0.1:{port}")];
    for host in lan_hosts {
        let url = format!("http://{host}:{port}");
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    if let IpAddr::V4(ip) = address.ip() {
        if ip != Ipv4Addr::UNSPECIFIED && !ip.is_loopback() {
            let url = format!("http://{ip}:{port}");
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_lan_origins_are_allowed_and_public_hosts_are_not() {
        assert!(lan_origin_allowed("http://192.168.1.20:3337"));
        assert!(lan_origin_allowed("http://10.0.0.8"));
        assert!(lan_origin_allowed("http://localhost:3338"));
        assert!(lan_origin_allowed("http://127.0.0.1:3337"));
        assert!(!lan_origin_allowed("http://example.com"));
        assert!(!lan_origin_allowed("http://8.8.8.8"));
        assert!(!lan_origin_allowed("file://"));
    }

    #[test]
    fn qc_paths_cannot_escape_the_web_root() {
        let root = PathBuf::from("/tmp/qc-web");
        assert!(contained(&root, "../secret").is_none());
        assert_eq!(
            contained(&root, "assets/AssetManifest.bin"),
            Some(root.join("assets/AssetManifest.bin"))
        );
    }
}

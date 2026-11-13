use crate::{Store, model::Evidence};
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use std::{io, net::SocketAddr, path::PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_stream::wrappers::ReceiverStream;

type Result<T> = std::result::Result<T, ApiError>;
pub struct ApiError(io::Error);
impl From<io::Error> for ApiError {
    fn from(value: io::Error) -> Self {
        Self(value)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.kind() {
            io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({"error":self.0.to_string()}))).into_response()
    }
}
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| ApiError(io::Error::other(error)))?
        .map_err(ApiError)
}

pub async fn index_page() -> Html<&'static str> {
    Html(include_str!("web/index.html"))
}

pub fn router(store: Store) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status":"ok","service":"robot-report"})) }),
        )
        .route("/", get(index_page))
        .route(
            "/assets/report.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("web/report.css"),
                )
            }),
        )
        .route(
            "/assets/history.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("web/history.js"),
                )
            }),
        )
        .route(
            "/assets/report.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("web/report.js"),
                )
            }),
        )
        .route("/api/capabilities", get(capabilities))
        .route("/api/projects", get(projects).post(create_project))
        .route("/api/projects/{project}/runs", get(runs).post(import_run))
        .route("/api/projects/{project}/runs/{run}", get(run))
        .route(
            "/api/projects/{project}/runs/{run}/cases/{case}/files/{file}",
            get(evidence),
        )
        .layer(middleware::from_fn(boundaries))
        .with_state(store)
}

fn can_manage(address: SocketAddr, headers: &HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    address.ip().is_loopback()
        && (host == "localhost"
            || host.starts_with("localhost:")
            || host == "127.0.0.1"
            || host.starts_with("127.0.0.1:")
            || host == "[::1]"
            || host.starts_with("[::1]:"))
}
async fn boundaries(request: Request, next: Next) -> Response {
    let read_only =
        request.method() == axum::http::Method::GET || request.method() == axum::http::Method::HEAD;
    if !read_only {
        let address = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        let headers = request.headers();
        let same_origin = headers.get(header::ORIGIN).is_none_or(|origin| {
            headers
                .get(header::HOST)
                .and_then(|host| host.to_str().ok())
                .is_some_and(|host| origin.to_str().ok() == Some(format!("http://{host}").as_str()))
        });
        if !address.is_some_and(|addr| can_manage(addr, headers)) || !same_origin {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Thêm project và nhập history tại localhost trên máy chủ."})),
            )
                .into_response();
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; media-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"));
    response
}
async fn capabilities(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    Json(json!({"canManage":can_manage(address, &headers)}))
}
async fn projects(State(store): State<Store>) -> Result<Json<Vec<crate::model::Project>>> {
    Ok(Json(blocking(move || store.projects()).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewProject {
    name: String,
    source_dir: PathBuf,
}
async fn create_project(
    State(store): State<Store>,
    Json(input): Json<NewProject>,
) -> Result<impl IntoResponse> {
    Ok((
        StatusCode::CREATED,
        Json(blocking(move || store.create_project(&input.name, &input.source_dir)).await?),
    ))
}
async fn runs(
    State(store): State<Store>,
    Path(project): Path<String>,
) -> Result<Json<Vec<crate::model::RunSummary>>> {
    Ok(Json(blocking(move || store.runs(&project)).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewRun {
    name: String,
    source_dir: PathBuf,
}
async fn import_run(
    State(store): State<Store>,
    Path(project): Path<String>,
    Json(input): Json<NewRun>,
) -> Result<impl IntoResponse> {
    Ok((
        StatusCode::CREATED,
        Json(blocking(move || store.import_run(&project, &input.name, &input.source_dir)).await?),
    ))
}
async fn run(
    State(store): State<Store>,
    Path((project, run)): Path<(String, String)>,
) -> Result<Json<crate::model::Run>> {
    Ok(Json(blocking(move || store.run(&project, &run)).await?))
}
#[derive(Default, Deserialize)]
struct FileQuery {
    #[serde(default)]
    download: bool,
}
async fn evidence(
    State(store): State<Store>,
    Path((project, run, case, file)): Path<(String, String, usize, usize)>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<Response> {
    let (path, evidence) = blocking(move || store.evidence(&project, &run, case, file)).await?;
    serve_file(path, evidence, query.download, headers).await
}
fn byte_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if size == 0 || end.contains(',') {
        return None;
    }
    if start.is_empty() {
        let suffix: u64 = end.parse().ok()?;
        return (suffix > 0).then_some((size.saturating_sub(suffix), size - 1));
    }
    let start: u64 = start.parse().ok()?;
    let end: u64 = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    (start < size && start <= end).then_some((start, end))
}
async fn serve_file(
    path: PathBuf,
    evidence: Evidence,
    download: bool,
    headers: HeaderMap,
) -> Result<Response> {
    let mut file = tokio::fs::File::open(&path).await?;
    let size = file.metadata().await?.len();
    let range = if let Some(value) = headers.get(header::RANGE) {
        match value
            .to_str()
            .ok()
            .and_then(|value| byte_range(value, size))
        {
            Some(range) => Some(range),
            None => {
                return Ok((
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, format!("bytes */{size}"))],
                )
                    .into_response());
            }
        }
    } else {
        None
    };
    let (start, length) = range
        .map(|(start, end)| (start, end - start + 1))
        .unwrap_or((0, size));
    file.seek(io::SeekFrom::Start(start)).await?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<io::Result<Vec<u8>>>(2);
    tokio::spawn(async move {
        let mut remaining = length;
        while remaining > 0 {
            let mut bytes = vec![0; remaining.min(64 * 1024) as usize];
            match file.read(&mut bytes).await {
                Ok(0) => {
                    let _ = sender
                        .send(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Evidence bị cắt ngắn",
                        )))
                        .await;
                    break;
                }
                Ok(count) => {
                    bytes.truncate(count);
                    remaining -= count as u64;
                    if sender.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    break;
                }
            }
        }
    });
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(receiver)));
    if let Some((start, end)) = range {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}"))
                .map_err(io::Error::other)?,
        );
    }
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).map_err(io::Error::other)?,
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    let name = evidence.name.rsplit('/').next().unwrap_or("evidence");
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{}; filename*=UTF-8''{}",
            if download { "attachment" } else { "inline" },
            percent_encode(name)
        ))
        .map_err(io::Error::other)?,
    );
    Ok(response)
}
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        _ => "text/plain; charset=utf-8",
    }
}

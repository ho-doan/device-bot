//! Acquire a CI artifact into project-owned temporary storage.
use crate::{Project, invalid};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

pub(crate) const DRY_RUN_PLACEHOLDER: &str = "__DEVICE_BOT_ARTIFACT_DRY_RUN__";

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DownloadAuth {
    GitlabJobToken {
        token_env: String,
        #[serde(default = "default_gitlab_origin_env")]
        origin_env: String,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Extraction {
    Zip {
        #[serde(default = "default_expanded_bytes")]
        max_expanded_bytes: u64,
        #[serde(default = "default_archive_entries")]
        max_entries: usize,
    },
}

fn default_expanded_bytes() -> u64 {
    4 * 1024 * 1024 * 1024
}
fn default_archive_entries() -> usize {
    20_000
}

impl Extraction {
    pub(crate) fn validate(&self) -> io::Result<()> {
        match self {
            Self::Zip {
                max_expanded_bytes,
                max_entries,
            } if *max_expanded_bytes > 0
                && *max_expanded_bytes <= 10 * 1024 * 1024 * 1024
                && *max_entries > 0
                && *max_entries <= 100_000 =>
            {
                Ok(())
            }
            _ => Err(invalid("ZIP extraction limits are invalid")),
        }
    }
}

fn default_gitlab_origin_env() -> String {
    "CI_SERVER_URL".into()
}

#[derive(Clone)]
pub(crate) struct FilePreparation {
    pub(crate) path_env: String,
    pub(crate) sha256_env: Option<String>,
    pub(crate) file_name: String,
    pub(crate) max_bytes: u64,
    pub(crate) auth: Option<DownloadAuth>,
    pub(crate) extract: Option<Extraction>,
}

pub(crate) struct Acquired {
    pub(crate) path: PathBuf,
    // The temporary directory remains owned until the project finishes.
    _directory: tempfile::TempDir,
}

struct ExtractionCancellation(Arc<AtomicBool>);
impl Drop for ExtractionCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl FilePreparation {
    fn download_token(
        &self,
        project: &Project,
        artifact_url: &Url,
    ) -> io::Result<Option<reqwest::header::HeaderValue>> {
        let Some(DownloadAuth::GitlabJobToken {
            token_env,
            origin_env,
        }) = &self.auth
        else {
            return Ok(None);
        };
        let configured = project.environment_value(origin_env).ok_or_else(|| {
            invalid(format!(
                "missing GitLab origin environment variable {origin_env}"
            ))
        })?;
        let allowed =
            Url::parse(&configured).map_err(|_| invalid("configured GitLab origin is invalid"))?;
        if !matches!(allowed.scheme(), "http" | "https")
            || allowed.host_str().is_none()
            || !allowed.username().is_empty()
            || allowed.password().is_some()
            || allowed.query().is_some()
            || allowed.fragment().is_some()
            || allowed.origin() != artifact_url.origin()
        {
            return Err(invalid(
                "artifact URL does not match the configured GitLab origin",
            ));
        }
        let token = project.environment_value(token_env).ok_or_else(|| {
            invalid(format!(
                "missing artifact authentication environment variable {token_env}"
            ))
        })?;
        if token.trim().is_empty() {
            return Err(invalid("artifact authentication token is empty"));
        }
        let mut value = reqwest::header::HeaderValue::from_str(&token)
            .map_err(|_| invalid("artifact authentication token is invalid"))?;
        value.set_sensitive(true);
        Ok(Some(value))
    }

    fn expected_sha256(&self, project: &Project) -> io::Result<Option<String>> {
        Ok(match &self.sha256_env {
            None => None,
            Some(name) => {
                let value = project.environment_value(name).ok_or_else(|| {
                    invalid(format!(
                        "missing artifact checksum environment variable {name}"
                    ))
                })?;
                if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(invalid(
                        "artifact SHA-256 must contain 64 hexadecimal characters",
                    ));
                }
                Some(value.to_ascii_lowercase())
            }
        })
    }

    pub(crate) fn validate_source(&self, project: &Project, source: &str) -> io::Result<()> {
        self.expected_sha256(project)?;
        if source.starts_with("https://") || source.starts_with("http://") {
            let url = Url::parse(source).map_err(|_| invalid("invalid artifact URL"))?;
            if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                return Err(invalid("artifact URL must not contain credentials"));
            }
            self.download_token(project, &url)?;
        } else {
            if source.contains("://") {
                return Err(invalid(
                    "artifact source must be a file path or HTTP(S) URL",
                ));
            }
            let metadata = std::fs::metadata(project.root.join(source))
                .map_err(|_| invalid("artifact file cannot be opened"))?;
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > self.max_bytes {
                return Err(invalid(
                    "artifact must be a nonempty regular file within the configured size limit",
                ));
            }
        }
        Ok(())
    }

    pub(crate) async fn acquire(
        &self,
        project: &Project,
        source: &str,
        evidence: &Path,
    ) -> io::Result<Acquired> {
        let expected = self.expected_sha256(project)?;
        let directory = tempfile::Builder::new()
            .prefix("device-bot-artifact-")
            .tempdir()?;
        let path = directory.path().join(&self.file_name);
        let mut output = tokio::fs::File::create(&path).await?;
        let mut digest = Sha256::new();
        let mut size = 0u64;
        let source_kind;
        if source.starts_with("https://") || source.starts_with("http://") {
            source_kind = "url";
            let url = Url::parse(source).map_err(|_| invalid("invalid artifact URL"))?;
            if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                return Err(invalid("artifact URL must not contain credentials"));
            }
            let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| io::Error::other("could not configure artifact TLS"))?
            .with_platform_verifier()
            .map_err(|_| io::Error::other("could not initialize artifact TLS verifier"))?
            .with_no_client_auth();
            let client = reqwest::Client::builder()
                .use_preconfigured_tls(tls)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(600))
                .build()
                .map_err(|_| io::Error::other("could not configure artifact downloader"))?;
            let token = self.download_token(project, &url)?;
            let origin = url.origin();
            let mut current = url;
            let mut redirects = 0;
            let mut response = loop {
                let mut request = client.get(current.clone());
                if current.origin() == origin {
                    if let Some(token) = &token {
                        request = request.header("JOB-TOKEN", token.clone());
                    }
                }
                let response = request
                    .send()
                    .await
                    .map_err(|_| io::Error::other("artifact download failed"))?;
                if response.status().is_redirection() {
                    if redirects >= 5 {
                        return Err(invalid("artifact redirect limit exceeded"));
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or_else(|| invalid("artifact redirect is invalid"))?;
                    let next = current
                        .join(location)
                        .map_err(|_| invalid("artifact redirect is invalid"))?;
                    if !matches!(next.scheme(), "http" | "https")
                        || next.host_str().is_none()
                        || !next.username().is_empty()
                        || next.password().is_some()
                        || (current.scheme() == "https" && next.scheme() != "https")
                    {
                        return Err(invalid("artifact redirect rejected"));
                    }
                    current = next;
                    redirects += 1;
                    continue;
                }
                break response
                    .error_for_status()
                    .map_err(|_| io::Error::other("artifact download returned an error status"))?;
            };
            if response
                .content_length()
                .is_some_and(|length| length > self.max_bytes)
            {
                return Err(invalid("artifact exceeds configured size limit"));
            }
            while let Some(bytes) = response
                .chunk()
                .await
                .map_err(|_| io::Error::other("artifact download interrupted"))?
            {
                size = append(&mut output, &mut digest, size, &bytes, self.max_bytes).await?;
            }
        } else {
            source_kind = "file";
            if source.contains("://") {
                return Err(invalid(
                    "artifact source must be a file path or HTTP(S) URL",
                ));
            }
            let source = project.root.join(source);
            let mut input = tokio::fs::File::open(source)
                .await
                .map_err(|_| invalid("artifact file cannot be opened"))?;
            let metadata = input
                .metadata()
                .await
                .map_err(|_| invalid("artifact file metadata is unavailable"))?;
            if !metadata.is_file() || metadata.len() > self.max_bytes {
                return Err(invalid(
                    "artifact must be a regular file within the configured size limit",
                ));
            }
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let count = input
                    .read(&mut buffer)
                    .await
                    .map_err(|_| io::Error::other("could not read artifact file"))?;
                if count == 0 {
                    break;
                }
                size = append(
                    &mut output,
                    &mut digest,
                    size,
                    &buffer[..count],
                    self.max_bytes,
                )
                .await?;
            }
        }
        if size == 0 {
            return Err(invalid("artifact is empty"));
        }
        output.flush().await?;
        drop(output);
        let actual = format!("{:x}", digest.finalize());
        if expected.is_some_and(|expected| expected != actual) {
            return Err(invalid(
                "artifact SHA-256 does not match the expected checksum",
            ));
        }
        let (directory, path) = match self.extract.clone() {
            None => (directory, path),
            Some(extraction) => {
                let cancellation = ExtractionCancellation(Arc::new(AtomicBool::new(false)));
                let cancelled = Arc::clone(&cancellation.0);
                let result = tokio::task::spawn_blocking(move || {
                    let extracted = directory.path().join(format!(
                        "{}.unpacked",
                        path.file_name().unwrap().to_string_lossy()
                    ));
                    extract_archive(&path, &extracted, &extraction, &cancelled)?;
                    Ok::<_, io::Error>((directory, extracted))
                })
                .await
                .map_err(|_| io::Error::other("artifact extraction task failed"))??;
                drop(cancellation);
                result
            }
        };
        let metadata = serde_json::json!({"source": source_kind, "bytes": size, "sha256": actual, "checksum_verified": self.sha256_env.is_some()});
        tokio::fs::write(
            evidence.join("artifact.json"),
            serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?,
        )
        .await?;
        Ok(Acquired {
            path,
            _directory: directory,
        })
    }
}

fn extract_archive(
    source: &Path,
    destination: &Path,
    extraction: &Extraction,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    use std::io::{Read, Write};
    let Extraction::Zip {
        max_expanded_bytes,
        max_entries,
    } = extraction;
    let input = std::fs::File::open(source).map_err(|_| invalid("cannot open artifact ZIP"))?;
    let mut archive = zip::ZipArchive::new(input).map_err(|_| invalid("invalid artifact ZIP"))?;
    if archive.is_empty() || archive.len() > *max_entries {
        return Err(invalid("artifact ZIP entry count exceeds configured limit"));
    }
    std::fs::create_dir(destination)?;
    let mut remaining = *max_expanded_bytes;
    for index in 0..archive.len() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "artifact extraction interrupted",
            ));
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid("invalid artifact ZIP entry"))?;
        let name = entry.name();
        if name.contains('\\')
            || name.contains(':')
            || name.contains('\0')
            || name.split('/').any(|part| part == "." || part == "..")
        {
            return Err(invalid("unsafe artifact ZIP path"));
        }
        let relative = entry
            .enclosed_name()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| invalid("unsafe artifact ZIP path"))?;
        let mode = entry.unix_mode().unwrap_or(0);
        let kind = mode & 0o170000;
        if !matches!(kind, 0 | 0o100000 | 0o040000) {
            return Err(invalid("artifact ZIP contains a link or special file"));
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            if entry.size() != 0 {
                return Err(invalid("invalid artifact ZIP directory"));
            }
            std::fs::create_dir_all(output)?;
            continue;
        }
        if entry.size() > remaining {
            return Err(invalid("artifact ZIP exceeds expanded size limit"));
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|_| invalid("artifact ZIP has conflicting paths"))?;
        let mut written = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "artifact extraction interrupted",
                ));
            }
            let count = entry
                .read(&mut buffer)
                .map_err(|_| invalid("could not extract artifact ZIP entry"))?;
            if count == 0 {
                break;
            }
            written = written
                .checked_add(count as u64)
                .filter(|size| *size <= remaining)
                .ok_or_else(|| invalid("artifact ZIP exceeds expanded size limit"))?;
            file.write_all(&buffer[..count])?;
        }
        if written > remaining || written != entry.size() {
            return Err(invalid(
                "artifact ZIP exceeds expanded size limit or has invalid data",
            ));
        }
        remaining -= written;
        #[cfg(unix)]
        if mode & 0o111 != 0 {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

async fn append(
    output: &mut tokio::fs::File,
    digest: &mut Sha256,
    size: u64,
    bytes: &[u8],
    max: u64,
) -> io::Result<u64> {
    let size = size
        .checked_add(bytes.len() as u64)
        .filter(|size| *size <= max)
        .ok_or_else(|| invalid("artifact exceeds configured size limit"))?;
    digest.update(bytes);
    output.write_all(bytes).await?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Fixture(tempfile::TempDir);
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            crate::init(root.path()).unwrap();
            Self(root)
        }
        fn project(&self) -> Project {
            Project::load(self.0.path()).unwrap()
        }
    }
    fn config() -> FilePreparation {
        FilePreparation {
            path_env: "APP_FILE".into(),
            sha256_env: None,
            file_name: "app.apk".into(),
            max_bytes: 1024,
            auth: None,
            extract: None,
        }
    }
    async fn request_headers(socket: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 16384 {
            let mut byte = [0u8; 1];
            if socket.read(&mut byte).await.unwrap() == 0 {
                break;
            }
            bytes.push(byte[0]);
        }
        String::from_utf8(bytes).unwrap()
    }
    async fn serve(response: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/download?token=private",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = request_headers(&mut socket).await;
            socket.write_all(response).await.unwrap();
        });
        (url, task)
    }

    #[tokio::test]
    async fn local_file_is_snapshotted_verified_and_cleaned_up() {
        let f = Fixture::new();
        let source = f.0.path().join("input.apk");
        std::fs::write(&source, b"original apk").unwrap();
        let sha = format!("{:x}", Sha256::digest(b"original apk"));
        std::fs::write(
            f.0.path().join(".env"),
            format!("ARTIFACT_CHECKSUM={sha}\n"),
        )
        .unwrap();
        let project = f.project();
        let config = FilePreparation {
            sha256_env: Some("ARTIFACT_CHECKSUM".into()),
            ..config()
        };
        config.validate_source(&project, "input.apk").unwrap();
        let acquired = config
            .acquire(&project, "input.apk", f.0.path())
            .await
            .unwrap();
        assert_eq!(acquired.path.file_name().unwrap(), "app.apk");
        std::fs::write(&source, b"changed").unwrap();
        assert_eq!(std::fs::read(&acquired.path).unwrap(), b"original apk");
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.0.path().join("artifact.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["sha256"], sha);
        assert_eq!(metadata["checksum_verified"], true);
        let staged = acquired.path.clone();
        drop(acquired);
        assert!(!staged.exists());
        assert!(source.exists());
    }

    #[tokio::test]
    async fn zip_extraction_stages_a_directory_and_cleans_it_up() {
        use std::io::Write;
        let f = Fixture::new();
        let source = f.0.path().join("app.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(source).unwrap());
        zip.start_file(
            "assets/index.html",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"fixture").unwrap();
        zip.finish().unwrap();
        let settings = FilePreparation {
            extract: Some(Extraction::Zip {
                max_expanded_bytes: 100,
                max_entries: 10,
            }),
            ..config()
        };
        let acquired = settings
            .acquire(&f.project(), "app.zip", f.0.path())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(acquired.path.join("assets/index.html")).unwrap(),
            b"fixture"
        );
        let staged = acquired.path.clone();
        drop(acquired);
        assert!(!staged.exists());
    }

    #[tokio::test]
    async fn zip_extraction_rejects_traversal_links_and_expansion_limits() {
        use std::io::Write;
        for case in ["traversal", "link", "expanded"] {
            let f = Fixture::new();
            let source = f.0.path().join("app.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(source).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            match case {
                "traversal" => {
                    zip.start_file("../escaped", options).unwrap();
                    zip.write_all(b"bad").unwrap();
                }
                "link" => {
                    zip.add_symlink("link", "../escaped", options).unwrap();
                }
                _ => {
                    zip.start_file("large", options).unwrap();
                    zip.write_all(b"too much data").unwrap();
                }
            }
            zip.finish().unwrap();
            let settings = FilePreparation {
                extract: Some(Extraction::Zip {
                    max_expanded_bytes: if case == "expanded" { 4 } else { 100 },
                    max_entries: 10,
                }),
                ..config()
            };
            let error = settings
                .acquire(&f.project(), "app.zip", f.0.path())
                .await
                .err()
                .unwrap()
                .to_string();
            let expected = match case {
                "traversal" => "unsafe",
                "link" => "link",
                _ => "size limit",
            };
            assert!(error.contains(expected), "{case}: {error}");
            assert!(!f.0.path().join("artifact.json").exists());
        }
    }

    #[test]
    fn zip_extraction_observes_cancellation() {
        use std::io::Write;
        let f = Fixture::new();
        let source = f.0.path().join("app.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
        zip.start_file("entry", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"contents").unwrap();
        zip.finish().unwrap();
        let cancelled = AtomicBool::new(true);
        let options = Extraction::Zip {
            max_expanded_bytes: 100,
            max_entries: 10,
        };
        let error =
            extract_archive(&source, &f.0.path().join("out"), &options, &cancelled).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(!f.0.path().join("out/entry").exists());
    }

    #[test]
    fn extraction_cancellation_is_signaled_when_the_run_is_dropped() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = ExtractionCancellation(Arc::clone(&cancelled));
        drop(guard);
        assert!(cancelled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn downloads_http_without_persisting_url_credentials_or_token() {
        let f = Fixture::new();
        let project = f.project();
        let (url, server) =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello").await;
        let acquired = config().acquire(&project, &url, f.0.path()).await.unwrap();
        server.await.unwrap();
        assert_eq!(std::fs::read(&acquired.path).unwrap(), b"hello");
        let metadata = std::fs::read_to_string(f.0.path().join("artifact.json")).unwrap();
        assert!(!metadata.contains("private") && !metadata.contains("127.0.0.1"));
        assert!(metadata.contains("\"source\": \"url\""));
    }

    #[tokio::test]
    async fn rejects_bad_checksum_oversized_body_and_non_http_redirect() {
        let f = Fixture::new();
        std::fs::write(
            f.0.path().join(".env"),
            format!("BAD_CHECKSUM={}\n", "0".repeat(64)),
        )
        .unwrap();
        let project = f.project();
        std::fs::write(f.0.path().join("input.apk"), b"contents").unwrap();
        let checksum = FilePreparation {
            sha256_env: Some("BAD_CHECKSUM".into()),
            ..config()
        };
        assert!(
            checksum
                .acquire(&project, "input.apk", f.0.path())
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("SHA-256")
        );
        assert!(!f.0.path().join("artifact.json").exists());
        let (url, server) = serve(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabcdef").await;
        let small = FilePreparation {
            max_bytes: 4,
            ..config()
        };
        assert!(
            small
                .acquire(&project, &url, f.0.path())
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("size limit")
        );
        server.await.unwrap();
        let (url, server) = serve(b"HTTP/1.1 302 Found\r\nLocation: file:///etc/passwd\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let message = config()
            .acquire(&project, &url, f.0.path())
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(!message.contains("private") && !message.contains("/etc/passwd"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn gitlab_job_token_is_not_forwarded_to_a_different_origin() {
        let f = Fixture::new();
        let sink = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination = format!("http://{}/artifact", sink.local_addr().unwrap());
        let sink_task = tokio::spawn(async move {
            let (mut socket, _) = sink.accept().await.unwrap();
            let request = request_headers(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata")
                .await
                .unwrap();
            request
        });
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/private", upstream.local_addr().unwrap());
        std::fs::write(
            f.0.path().join(".env"),
            format!(
                "CI_TEST_JOB_TOKEN=example-private-token\nCI_TEST_GITLAB_ORIGIN=http://{}\n",
                upstream.local_addr().unwrap()
            ),
        )
        .unwrap();
        let project = f.project();
        let upstream_task = tokio::spawn(async move {
            let (mut socket, _) = upstream.accept().await.unwrap();
            let request = request_headers(&mut socket).await;
            socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            request
        });
        let config = FilePreparation {
            auth: Some(DownloadAuth::GitlabJobToken {
                token_env: "CI_TEST_JOB_TOKEN".into(),
                origin_env: "CI_TEST_GITLAB_ORIGIN".into(),
            }),
            ..config()
        };
        assert!(
            config
                .validate_source(&project, "http://untrusted.example/artifact")
                .is_err()
        );
        config.validate_source(&project, &url).unwrap();
        let acquired = config.acquire(&project, &url, f.0.path()).await.unwrap();
        assert_eq!(std::fs::read(&acquired.path).unwrap(), b"data");
        let first = upstream_task.await.unwrap().to_ascii_lowercase();
        let second = sink_task.await.unwrap().to_ascii_lowercase();
        assert!(first.contains("job-token: example-private-token"));
        assert!(!second.contains("job-token") && !second.contains("example-private-token"));
        assert!(
            !std::fs::read_to_string(f.0.path().join("artifact.json"))
                .unwrap()
                .contains("example-private-token")
        );
    }
}

#![cfg(test)]
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use robot_report::{Store, model::CaseReport};
use serde_json::{Value, json};
use std::{fs, net::SocketAddr, path::PathBuf};
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("qa-report-test-{}", Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        let store = Store::open(root.join("history")).unwrap();
        Self {
            root,
            source,
            store,
        }
    }
    fn case(&self, folder: &str, status: &str) -> PathBuf {
        let path = self.source.join(folder).join("case-1");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("report.json"), serde_json::to_vec(&json!({
            "name":"IT-01", "status":status, "dry_run":false,
            "selected_device":{"name":"Test Simulator","id":"test-1","platform":"ios"},
            "steps":[{"id":"it-01-04.replacement.step-3","action":"if_element","status":if status == "FAIL" {"failed"} else {"passed"},"error":if status == "FAIL" {Some("ambiguous selector <script>alert(1)</script>")} else {None}}],
            "error":if status == "FAIL" {Some("step it-01-04: ambiguous selector")} else {None},
            "error_kind":if status == "FAIL" {Some("execution")} else {None},
            "limitations":if status == "PARTIAL" {vec!["Widget property unavailable"]} else {vec![]}
        })).unwrap()).unwrap();
        fs::write(path.join("video.mp4"), b"0123456789").unwrap();
        fs::write(path.join("result.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        fs::write(path.join("mcp-calls.jsonl"), b"{\"tool\":\"query\"}\n").unwrap();
        fs::write(path.parent().unwrap().join("stderr.log"), b"case stderr\n").unwrap();
        path.parent().unwrap().to_path_buf()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn request(uri: &str, remote: bool) -> axum::http::request::Builder {
    Request::builder()
        .uri(uri)
        .header("host", "127.0.0.1:8765")
        .extension(ConnectInfo(
            if remote {
                "192.0.2.10:1234"
            } else {
                "127.0.0.1:1234"
            }
            .parse::<SocketAddr>()
            .unwrap(),
        ))
}

#[test]
fn history_survives_restart_source_deletion_and_repeated_case_names() {
    let f = Fixture::new();
    let source = f.case("run", "FAIL");
    let project = f.store.create_project("Orbit", &f.source).unwrap();
    let failed = f.store.import_run(&project.id, "Before", &source).unwrap();
    f.case("run", "PASS");
    let passed = f.store.import_run(&project.id, "After", &source).unwrap();
    assert_ne!(failed.id, passed.id);
    fs::remove_dir_all(&f.source).unwrap();
    let restarted = Store::open(f.store.root()).unwrap();
    assert_eq!(restarted.projects().unwrap().len(), 1);
    assert_eq!(restarted.runs(&project.id).unwrap().len(), 2);
    assert_eq!(
        restarted
            .run(&project.id, &failed.id)
            .unwrap()
            .counts
            .failed,
        1
    );
    assert_eq!(
        restarted
            .run(&project.id, &passed.id)
            .unwrap()
            .counts
            .passed,
        1
    );
    let run = restarted.run(&project.id, &failed.id).unwrap();
    let video = run.cases[0]
        .files
        .iter()
        .find(|file| file.kind == "video")
        .unwrap();
    assert_eq!(
        fs::read(
            restarted
                .evidence(&project.id, &failed.id, 0, video.id)
                .unwrap()
                .0
        )
        .unwrap(),
        b"0123456789"
    );
    assert!(
        run.cases[0]
            .files
            .iter()
            .any(|file| file.name == "stderr.log")
    );
    assert_eq!(
        run.cases[0].report.steps[0].id,
        "it-01-04.replacement.step-3"
    );
}

#[test]
fn multiple_projects_import_concurrently_without_mixing_history() {
    let f = Fixture::new();
    let source = f.case("run", "PARTIAL");
    let first = f.store.create_project("Mobile", &f.source).unwrap();
    let second = f.store.create_project("Web", &f.source).unwrap();
    let store = f.store.clone();
    let source2 = source.clone();
    let id = first.id.clone();
    let task = std::thread::spawn(move || store.import_run(&id, "Run mobile", &source2).unwrap());
    let run2 = f.store.import_run(&second.id, "Run web", &source).unwrap();
    let run1 = task.join().unwrap();
    assert_eq!(f.store.runs(&first.id).unwrap()[0].id, run1.id);
    assert_eq!(f.store.runs(&second.id).unwrap()[0].id, run2.id);
    assert_eq!(run1.counts.partial, 1);
    assert_eq!(run1.counts.passed, 0);
    assert!(f.store.run(&second.id, &run1.id).is_err());
}

#[test]
fn bad_import_is_atomic_and_cannot_escape_the_project_or_read_history_as_source() {
    let f = Fixture::new();
    let source = f.case("run", "PASS");
    let project = f.store.create_project("Orbit", &f.source).unwrap();
    assert!(f.store.import_run(&project.id, "outside", &f.root).is_err());
    assert!(f.store.run("../source", "../../secret").is_err());
    fs::write(source.join("case-1/report.json"), b"{broken").unwrap();
    assert!(f.store.import_run(&project.id, "broken", &source).is_err());
    assert!(f.store.runs(&project.id).unwrap().is_empty());
    assert!(!fs::read_dir(f.store.root()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".run-")
    }));
    let wide = f.store.create_project("Wide root", &f.root).unwrap();
    assert!(f.store.import_run(&wide.id, "recursive", &f.root).is_err());
}

#[cfg(unix)]
#[test]
fn source_symlinks_are_not_published() {
    let f = Fixture::new();
    let source = f.case("run", "PASS");
    let private = f.root.join("outside.log");
    fs::write(&private, b"private").unwrap();
    std::os::unix::fs::symlink(&private, source.join("case-1/leak.log")).unwrap();
    let project = f.store.create_project("Orbit", &f.source).unwrap();
    let summary = f.store.import_run(&project.id, "Run", &source).unwrap();
    assert!(
        !f.store.run(&project.id, &summary.id).unwrap().cases[0]
            .files
            .iter()
            .any(|file| file.name == "leak.log")
    );
}

#[test]
fn failure_and_dry_run_are_never_counted_as_pass() {
    let report: CaseReport =
        serde_json::from_value(json!({"name":"case", "status":"PASS", "dry_run":true})).unwrap();
    assert_eq!(report.effective_status(), "DRY_RUN");
    let report: CaseReport = serde_json::from_value(json!({"name":"case", "status":"PASS", "steps":[{"id":"nested.failed","action":"tap", "status":"failed"}]})).unwrap();
    assert_eq!(report.effective_status(), "FAIL");
}

#[tokio::test]
async fn http_exposes_history_failed_step_and_streams_original_video_ranges() {
    let f = Fixture::new();
    let source = f.case("run", "FAIL");
    let project = f.store.create_project("Orbit", &f.source).unwrap();
    let run = f
        .store
        .import_run(&project.id, "Failed run", &source)
        .unwrap();
    let data = f.store.run(&project.id, &run.id).unwrap();
    let video = data.cases[0]
        .files
        .iter()
        .find(|file| file.kind == "video")
        .unwrap();
    let app = robot_report::router(f.store.clone());
    let health = app
        .clone()
        .oneshot(request("/health", false).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let health: Value =
        serde_json::from_slice(&to_bytes(health.into_body(), 1000).await.unwrap()).unwrap();
    assert_eq!(health["service"], "robot-report");
    assert_eq!(health["status"], "ok");
    let path = format!("/api/projects/{}/runs/{}", project.id, run.id);
    let response = app
        .clone()
        .oneshot(request(&path, false).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let report: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(
        report["cases"][0]["report"]["steps"][0]["id"],
        "it-01-04.replacement.step-3"
    );
    assert!(
        report["cases"][0]["report"]["steps"][0]["error"]
            .as_str()
            .unwrap()
            .contains("<script>")
    );
    let path = format!("{path}/cases/0/files/{}", video.id);
    for (range, expected) in [
        ("bytes=2-5", b"2345".as_slice()),
        ("bytes=-3", b"789"),
        ("bytes=7-", b"789"),
    ] {
        let response = app
            .clone()
            .oneshot(
                request(&path, true)
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-type"], "video/mp4");
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
            expected
        );
    }
    let response = app
        .clone()
        .oneshot(
            request(&format!("{path}?download=true"), true)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
        b"0123456789"
    );
    let response = app
        .oneshot(
            request(&path, false)
                .header("range", "bytes=100-200")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()["content-range"], "bytes */10");
}

#[tokio::test]
async fn qa_read_access_cannot_mutate_projects_or_cross_project_history() {
    let f = Fixture::new();
    let app = robot_report::router(f.store.clone());
    let body = json!({"name":"Orbit","sourceDir":f.source});
    for (remote, origin) in [
        (true, "http://127.0.0.1:8765"),
        (false, "https://unrelated.example"),
    ] {
        let response = app
            .clone()
            .oneshot(
                request("/api/projects", remote)
                    .method("POST")
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = app
        .clone()
        .oneshot(
            request("/api/projects", false)
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = app
        .oneshot(request("/api/projects", true).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let projects: Vec<Value> =
        serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap()).unwrap();
    assert_eq!(projects.len(), 1);
}

#[test]
fn cli_rejects_unknown_commands_and_missing_required_flags() {
    let fixture = Fixture::new();
    for args in [
        vec!["publish"],
        vec!["nonsense"],
        vec!["start", "--source", "x"],
    ] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_robot-report"))
            .args(args)
            .arg("--data-dir")
            .arg(fixture.root.join("cli-history"))
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
}

#[test]
fn version_does_not_start_the_service_or_create_history() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_robot-report"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        env!("CARGO_PKG_VERSION")
    );
    let invalid = std::process::Command::new(env!("CARGO_BIN_EXE_robot-report"))
        .args(["--version", "extra"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
}

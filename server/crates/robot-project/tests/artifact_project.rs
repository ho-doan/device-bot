#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

#[tokio::test]
async fn project_stages_one_artifact_for_all_flows_and_removes_it_afterward() {
    let root = tempfile::tempdir().unwrap();
    robot_project::init(root.path()).unwrap();
    fs::write(root.path().join("input.apk"), b"test apk payload").unwrap();
    fs::write(
        root.path().join("flows/second.yaml"),
        "version: 2\nname: second\nsteps:\n  - wait: {ms: 0}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("profiles.yaml"),
        "version: 1\ntargets:\n  android: {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("project.yaml"),
        r#"version: 1
kind: project
name: artifact-test
flows: [flows]
platforms:
  android:
    profiles: profiles.yaml
    app_id: '${env:APP_APK}'
    prepare: {kind: artifact_file, path_env: APP_APK, file_name: test.apk}
"#,
    )
    .unwrap();
    let runner = root.path().join("runner.sh");
    fs::write(
        &runner,
        r#"#!/bin/sh
test "$(cat "$APP_APK")" = 'test apk payload' || exit 91
printf '%s\n' "$APP_APK" >> witness.txt
while [ "$#" -gt 0 ]; do
  case "$1" in
    --artifacts) output="$2"; shift 2 ;;
    --app-id) app="$2"; shift 2 ;;
    *) shift ;;
  esac
done
test "$app" = "$APP_APK" || exit 92
mkdir -p "$output/case-001"
printf '<testsuite tests="1"/>' > "$output/case-001/junit.xml"
"#,
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    let project = robot_project::Project::load(root.path()).unwrap();
    let args = ["--platform", "android", "--artifact", "input.apk"].map(str::to_owned);
    let status = robot_project::run(&project, &args, &runner, &runner, root.path())
        .await
        .unwrap();
    assert_eq!(status, 0);
    let witness = fs::read_to_string(root.path().join("witness.txt")).unwrap();
    let paths: Vec<_> = witness.lines().collect();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], paths[1]);
    assert!(paths[0].ends_with("test.apk"));
    assert!(!std::path::Path::new(paths[0]).exists());
    assert_eq!(
        fs::read(root.path().join("input.apk")).unwrap(),
        b"test apk payload"
    );
    let runs: Vec<_> = fs::read_dir(root.path().join("artifacts/project"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(runs.len(), 1);
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(runs[0].join("artifact.json")).unwrap()).unwrap();
    assert_eq!(metadata["bytes"], 16);
    assert_eq!(metadata["source"], "file");
}

#[tokio::test]
async fn project_exposes_extracted_zip_to_all_cases_and_removes_it_afterward() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    robot_project::init(root.path()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(root.path().join("app.zip")).unwrap());
    zip.start_file("index.html", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"web build").unwrap();
    zip.finish().unwrap();
    fs::write(
        root.path().join("profiles.yaml"),
        "version: 1\ntargets: {web: {}}\n",
    )
    .unwrap();
    fs::write(root.path().join("project.yaml"), "version: 1\nkind: project\nname: zip\nflows: [flows]\nplatforms:\n  web:\n    profiles: profiles.yaml\n    prepare:\n      kind: artifact_file\n      path_env: APP_DIR\n      file_name: build.zip\n      extract: {kind: zip, max_expanded_bytes: 1024}\n").unwrap();
    let runner = root.path().join("runner.sh");
    fs::write(&runner, "#!/bin/sh\ntest \"$(cat \"$APP_DIR/index.html\")\" = 'web build' || exit 91\nprintf '%s' \"$APP_DIR\" > witness\nwhile [ $# -gt 0 ]; do if [ \"$1\" = --artifacts ]; then output=$2; shift 2; else shift; fi; done\nmkdir -p \"$output/case-1\"\nprintf '<testsuite/>' > \"$output/case-1/junit.xml\"\n").unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    let project = robot_project::Project::load(root.path()).unwrap();
    let args = ["--platform", "web", "--artifact", "app.zip"].map(str::to_owned);
    assert_eq!(
        robot_project::run(&project, &args, &runner, &runner, root.path())
            .await
            .unwrap(),
        0
    );
    let path = fs::read_to_string(root.path().join("witness")).unwrap();
    assert!(!std::path::Path::new(&path).exists());
}

#[tokio::test]
async fn project_serves_the_zip_during_flows_and_closes_it_afterward() {
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    robot_project::init(root.path()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(root.path().join("web.zip")).unwrap());
    zip.start_file("index.html", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"<base href='/app/'>project fixture")
        .unwrap();
    zip.finish().unwrap();
    fs::write(
        root.path().join("profiles.yaml"),
        "version: 1\ntargets: {chrome: {}}\n",
    )
    .unwrap();
    fs::write(root.path().join("project.yaml"), "version: 1\nkind: project\nname: web\nflows: [flows]\nplatforms:\n  web:\n    target: chrome\n    profiles: profiles.yaml\n    app_id: '${env:APP_URL}/route'\n    prepare:\n      kind: artifact_file\n      path_env: APP_DIR\n      file_name: web.zip\n      extract: {kind: zip}\n      serve_web: {base_url_env: APP_URL}\n").unwrap();
    let runner = root.path().join("runner.sh");
    fs::write(&runner, "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --artifacts) output=$2; shift 2;; --app-id) app=$2; shift 2;; *) shift;; esac; done\ntest \"$app\" = \"$APP_URL/route\" || exit 91\nprintf '%s' \"$APP_URL\" > witness.tmp\nmv witness.tmp witness\nwhile [ ! -f proceed ]; do sleep 0.05; done\nmkdir -p \"$output/case-1\"\nprintf '<testsuite/>' > \"$output/case-1/junit.xml\"\n").unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    let project = robot_project::Project::load(root.path()).unwrap();
    let args = ["--platform", "web", "--artifact", "web.zip"].map(str::to_owned);
    let reports = root.path().to_owned();
    let mut run = tokio::spawn(async move {
        robot_project::run(&project, &args, &runner, &runner, &reports).await
    });
    let witness = root.path().join("witness");
    let ready = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while !witness.is_file() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });
    tokio::select! {
        result = &mut run => panic!("project ended before serving the fixture: {result:?}"),
        result = ready => result.expect("project did not start serving the fixture"),
    }
    let url = url::Url::parse(&fs::read_to_string(witness).unwrap()).unwrap();
    assert_eq!(url.path(), "/app");
    let port = url.port().unwrap();
    let mut socket = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    socket
        .write_all(b"GET /app/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    assert!(response.contains("project fixture"));
    fs::write(root.path().join("proceed"), "").unwrap();
    assert_eq!(run.await.unwrap().unwrap(), 0);
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

#[tokio::test]
async fn dry_run_does_not_download_remote_artifacts() {
    let root = tempfile::tempdir().unwrap();
    robot_project::init(root.path()).unwrap();
    fs::write(
        root.path().join("profiles.yaml"),
        "version: 1\ntargets:\n  android: {}\n",
    )
    .unwrap();
    fs::write(root.path().join("project.yaml"), "version: 1\nkind: project\nname: dry\nflows: [flows]\nplatforms:\n  android:\n    profiles: profiles.yaml\n    prepare: {kind: artifact_file, path_env: APP_APK, file_name: app.apk}\n").unwrap();
    let runner = root.path().join("runner.sh");
    fs::write(
        &runner,
        r#"#!/bin/sh
test "$APP_APK" = '__DEVICE_BOT_ARTIFACT_DRY_RUN__' || exit 91
while [ "$#" -gt 0 ]; do
  if [ "$1" = --artifacts ]; then output="$2"; shift 2; else shift; fi
done
mkdir -p "$output/case-001"
printf '<testsuite tests="1"/>' > "$output/case-001/junit.xml"
"#,
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    let project = robot_project::Project::load(root.path()).unwrap();
    let args = [
        "--platform",
        "android",
        "--artifact",
        "http://127.0.0.1:9/unreachable.apk",
        "--dry-run",
    ]
    .map(str::to_owned);
    assert_eq!(
        robot_project::run(&project, &args, &runner, &runner, root.path())
            .await
            .unwrap(),
        0
    );
    let run = fs::read_dir(root.path().join("artifacts/project"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!run.join("artifact.json").exists());
}

#[tokio::test]
async fn invalid_artifact_setup_writes_junit_without_running_a_flow() {
    let root = tempfile::tempdir().unwrap();
    robot_project::init(root.path()).unwrap();
    fs::write(
        root.path().join("profiles.yaml"),
        "version: 1\ntargets:\n  android: {}\n",
    )
    .unwrap();
    fs::write(root.path().join("project.yaml"), "version: 1\nkind: project\nname: missing\nflows: [flows]\nplatforms:\n  android:\n    profiles: profiles.yaml\n    prepare: {kind: artifact_file, path_env: APP_APK, file_name: app.apk}\n").unwrap();
    let project = robot_project::Project::load(root.path()).unwrap();
    let output = root.path().join("ci-results");
    let args = vec![
        "--platform".into(),
        "android".into(),
        "--artifact".into(),
        "missing.apk".into(),
        "--artifacts".into(),
        output.to_string_lossy().into_owned(),
    ];
    let error = robot_project::run(&project, &args, root.path(), root.path(), root.path())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("artifact file"));
    let run = fs::read_dir(output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let junit = fs::read_to_string(run.join("project-error/setup/junit.xml")).unwrap();
    assert!(junit.contains("errors=\"1\""));
    assert!(!run.join("case-001").exists());
}

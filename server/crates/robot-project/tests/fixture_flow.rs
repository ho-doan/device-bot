#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt};

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("flows")).unwrap();
        for name in ["setup.yaml", "flows/a.yaml", "flows/b.yaml"] {
            fs::write(
                root.path().join(name),
                "version: 2\nname: smoke\nsteps: [{wait: {ms: 0}}]\n",
            )
            .unwrap();
        }
        fs::write(root.path().join("project.yaml"), "version: 1\nkind: project\nname: setup-test\nflows: [flows]\nbefore_each_flow: setup.yaml\n").unwrap();
        let runner = root.path().join("runner");
        fs::write(&runner, r##"#!/bin/sh
dry=false
while [ $# -gt 0 ]; do
  case "$1" in
    --artifacts) output=$2; shift 2 ;;
    --dry-run) dry=true; shift ;;
    *) flow=$1; shift ;;
  esac
done
name=$(basename "$flow")
printf '%s\n' "$name" >> calls
if [ "$dry" = false ]; then : > live-actions; fi
mkdir -p "$output/case-1"
printf '<testsuite/>' > "$output/case-1/junit.xml"
status=PASS
if [ "$name" = setup.yaml ] && [ -f fail-setup ]; then status=PARTIAL; fi
if [ "$name" = suite.yaml ] && [ -f fail-suite ]; then status=PARTIAL; fi
printf '{"status":"%s","dry_run":%s,"limitations":[]}' "$status" "$dry" > "$output/case-1/report.json"
"##).unwrap();
        fs::set_permissions(runner, fs::Permissions::from_mode(0o755)).unwrap();
        Self(root)
    }

    async fn run(&self, args: &[String]) -> std::io::Result<i32> {
        let root = self.0.path();
        let project = robot_project::Project::load(root).unwrap();
        robot_project::run(
            &project,
            args,
            &root.join("runner"),
            &root.join("runner"),
            root,
        )
        .await
    }
}

#[tokio::test]
async fn setup_flow_runs_before_each_case_and_partial_setup_stops_the_suite() {
    let f = Fixture::new();
    assert_eq!(f.run(&[]).await.unwrap(), 0);
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "setup.yaml\na.yaml\nsetup.yaml\nb.yaml\n"
    );
    fs::write(f.0.path().join("calls"), "").unwrap();
    fs::write(f.0.path().join("fail-setup"), "").unwrap();
    assert!(
        f.run(&[])
            .await
            .unwrap_err()
            .to_string()
            .contains("before_each_flow")
    );
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "setup.yaml\n"
    );
}

#[tokio::test]
async fn dry_run_compiles_setup_and_cases_without_executing_device_actions() {
    let f = Fixture::new();
    assert_eq!(f.run(&["--dry-run".into()]).await.unwrap(), 0);
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "setup.yaml\na.yaml\nsetup.yaml\nb.yaml\n"
    );
    assert!(!f.0.path().join("live-actions").exists());
}

#[tokio::test]
async fn platform_can_override_project_setup_flow() {
    let f = Fixture::new();
    fs::write(
        f.0.path().join("other.yaml"),
        "version: 2\nname: setup\nsteps: [{wait: {ms: 0}}]\n",
    )
    .unwrap();
    fs::write(
        f.0.path().join("profiles.yaml"),
        "version: 1\ntargets: {}\n",
    )
    .unwrap();
    let manifest = f.0.path().join("project.yaml");
    let mut source = fs::read_to_string(&manifest).unwrap();
    source.push_str(
        "platforms:\n  android:\n    profiles: profiles.yaml\n    before_each_flow: other.yaml\n",
    );
    fs::write(manifest, source).unwrap();
    assert_eq!(
        f.run(&["--platform".into(), "android".into()])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "other.yaml\na.yaml\nother.yaml\nb.yaml\n"
    );
}

#[tokio::test]
async fn suite_setup_runs_once_before_any_per_case_setup() {
    let f = Fixture::new();
    fs::write(
        f.0.path().join("suite.yaml"),
        "version: 2\nname: suite\nsteps: [{wait: {ms: 0}}]\n",
    )
    .unwrap();
    let manifest = f.0.path().join("project.yaml");
    let mut source = fs::read_to_string(&manifest).unwrap();
    source.push_str("before_all_flow: suite.yaml\n");
    fs::write(manifest, source).unwrap();
    assert_eq!(f.run(&[]).await.unwrap(), 0);
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "suite.yaml\nsetup.yaml\na.yaml\nsetup.yaml\nb.yaml\n"
    );
    fs::write(f.0.path().join("calls"), "").unwrap();
    fs::write(f.0.path().join("fail-suite"), "").unwrap();
    assert!(
        f.run(&[])
            .await
            .unwrap_err()
            .to_string()
            .contains("before_all_flow")
    );
    assert_eq!(
        fs::read_to_string(f.0.path().join("calls")).unwrap(),
        "suite.yaml\n"
    );
}

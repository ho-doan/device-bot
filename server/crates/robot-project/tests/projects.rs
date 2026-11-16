use robot_project::{Project, ensure_report, init, invocation};
use std::{fs, path::PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("robot-project-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn init_load_and_repeat_do_not_overwrite_existing_content() {
    let dir = Temp::new();
    let manifest = init(&dir.0).unwrap();
    let before = fs::read(&manifest).unwrap();
    let project = Project::load(&dir.0).unwrap();
    assert_eq!(project.name, "my-project");
    assert_eq!(project.flows, [project.root.join("flows/smoke.yaml")]);
    assert!(init(&dir.0).is_err());
    assert_eq!(fs::read(manifest).unwrap(), before);
}

#[test]
fn discovery_distinguishes_projects_paths_and_flow_options() {
    let dir = Temp::new();
    init(&dir.0).unwrap();
    let args = vec![
        "--artifacts".into(),
        dir.0.display().to_string(),
        "--dry-run".into(),
        dir.0.display().to_string(),
    ];
    let (_, forwarded) = invocation(&args).unwrap().unwrap();
    assert_eq!(forwarded, args[..3]);
    let flow = dir.0.join("flows/smoke.yaml").display().to_string();
    assert!(invocation(std::slice::from_ref(&flow)).unwrap().is_none());
    assert!(invocation(&[dir.0.display().to_string(), flow]).is_err());
}

#[test]
fn invalid_manifest_or_missing_flow_fails_before_runner_start() {
    let dir = Temp::new();
    let manifest = init(&dir.0).unwrap();
    let original = fs::read_to_string(&manifest).unwrap();
    for invalid in [
        original.replace("version: 1", "version: 99"),
        original.replace("flows/smoke.yaml", "flows/missing.yaml"),
        format!("{original}unknown: true\n"),
    ] {
        fs::write(&manifest, invalid).unwrap();
        assert!(Project::load(&manifest).is_err());
    }
}

#[test]
fn dotenv_loading_never_mutates_global_environment_or_leaks_bad_values() {
    let dir = Temp::new();
    init(&dir.0).unwrap();
    let key = format!("PROJECT_TEST_{}", uuid::Uuid::new_v4().simple());
    fs::write(dir.0.join(".env"), format!("{key}=project-only\n")).unwrap();
    Project::load(&dir.0).unwrap();
    assert!(std::env::var_os(key).is_none());
    fs::write(dir.0.join(".env"), "PASSWORD='must-not-appear-in-errors\n").unwrap();
    let error = Project::load(&dir.0).unwrap_err().to_string();
    assert!(!error.contains("must-not-appear"));
}

#[test]
fn fallback_preserves_real_case_reports_and_ignores_warmup() {
    let dir = Temp::new();
    fs::create_dir_all(dir.0.join("warmup/case-1")).unwrap();
    fs::write(dir.0.join("warmup/case-1/junit.xml"), "warmup").unwrap();
    assert!(ensure_report(&dir.0).unwrap());
    let before = fs::read(dir.0.join("setup/junit.xml")).unwrap();
    assert!(!ensure_report(&dir.0).unwrap());
    assert_eq!(fs::read(dir.0.join("setup/junit.xml")).unwrap(), before);
    let real = dir.0.join("real");
    fs::create_dir_all(real.join("case-1")).unwrap();
    fs::write(real.join("case-1/junit.xml"), "original").unwrap();
    assert!(!ensure_report(&real).unwrap());
    assert!(!real.join("setup").exists());
}

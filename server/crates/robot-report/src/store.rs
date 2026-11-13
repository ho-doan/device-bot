use crate::model::{Case, CaseReport, Counts, Evidence, Project, Run, RunSummary};
use crate::targets::classify;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_JSON: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_CASES: usize = 5_000;

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct FlowIngest {
    pub name: String,
    pub source: String,
    pub case_dir: PathBuf,
    pub app_id: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InboxManifest {
    source: Option<String>,
    name: Option<String>,
    app_id: Option<String>,
    url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunLink {
    project_id: String,
    run_id: String,
}

pub fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn now() -> u64 {
    millis(SystemTime::now())
}
fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn label(value: &str) -> io::Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 160 {
        return Err(invalid("Tên phải có từ 1 đến 160 ký tự"));
    }
    Ok(value.into())
}
fn uuid(value: &str) -> io::Result<&str> {
    Uuid::parse_str(value).map_err(|_| invalid("ID không hợp lệ"))?;
    Ok(value)
}
fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    if fs::metadata(path)?.len() > MAX_JSON {
        return Err(invalid("Tệp JSON vượt quá 8 MB"));
    }
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| invalid(format!("{}: {error}", path.display())))
}
fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(invalid("Metadata vượt quá 8 MB; chia nhỏ lượt import"));
    }
    fs::write(path, bytes)
}

/// A staging directory becomes visible only after all evidence and metadata exist.
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        fs::create_dir_all(root.as_ref().join("projects"))?;
        let store = Self {
            root: fs::canonicalize(root)?,
        };
        let _ = store.publish_inbox();
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn project_dir(&self, id: &str) -> io::Result<PathBuf> {
        Ok(self.root.join("projects").join(uuid(id)?))
    }
    pub fn create_project(&self, name: &str, source: &Path) -> io::Result<Project> {
        let source = fs::canonicalize(source)?;
        if !source.is_dir() || source.starts_with(&self.root) {
            return Err(invalid(
                "Nguồn project phải là thư mục evidence bên ngoài kho history",
            ));
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            name: label(name)?,
            source_dir: source.to_string_lossy().into_owned(),
            created_at: now(),
            origin: "import".into(),
        };
        let stage = Staging(self.root.join(format!(".project-{}", project.id)));
        fs::create_dir(&stage.0)?;
        fs::create_dir(stage.0.join("runs"))?;
        write_json(&stage.0.join("project.json"), &project)?;
        fs::rename(&stage.0, self.project_dir(&project.id)?)?;
        Ok(project)
    }
    pub fn project(&self, id: &str) -> io::Result<Project> {
        read_json(&self.project_dir(id)?.join("project.json"))
    }
    pub fn projects(&self) -> io::Result<Vec<Project>> {
        let mut projects = Vec::new();
        for entry in fs::read_dir(self.root.join("projects"))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.path().join("project.json").is_file() {
                projects.push(read_json::<Project>(&entry.path().join("project.json"))?);
            }
        }
        projects.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        Ok(projects)
    }
    pub fn runs(&self, project: &str) -> io::Result<Vec<RunSummary>> {
        self.project(project)?;
        let mut runs = Vec::new();
        for entry in fs::read_dir(self.project_dir(project)?.join("runs"))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.path().join("summary.json").is_file() {
                runs.push(read_json::<RunSummary>(&entry.path().join("summary.json"))?);
            }
        }
        runs.sort_by(|a, b| b.imported_at.cmp(&a.imported_at).then(b.id.cmp(&a.id)));
        Ok(runs)
    }
    fn run_dir(&self, project: &str, run: &str) -> io::Result<PathBuf> {
        Ok(self.project_dir(project)?.join("runs").join(uuid(run)?))
    }
    fn resolved_run_dir(&self, project: &str, run: &str) -> io::Result<PathBuf> {
        let dir = self.run_dir(project, run)?;
        let link = dir.join("link.json");
        if link.is_file() {
            let link: RunLink = read_json(&link)?;
            return self.run_dir(&link.project_id, &link.run_id);
        }
        Ok(dir)
    }
    pub fn run(&self, project: &str, run: &str) -> io::Result<Run> {
        self.project(project)?;
        let mut report = read_json::<Run>(&self.resolved_run_dir(project, run)?.join("run.json"))?;
        report.project_id = project.to_owned();
        Ok(report)
    }
    pub fn evidence(
        &self,
        project: &str,
        run: &str,
        case: usize,
        file: usize,
    ) -> io::Result<(PathBuf, Evidence)> {
        let report = self.run(project, run)?;
        let evidence = report
            .cases
            .iter()
            .find(|c| c.id == case)
            .and_then(|c| c.files.iter().find(|f| f.id == file))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Không tìm thấy evidence"))?;
        let root = fs::canonicalize(self.resolved_run_dir(project, run)?)?;
        let path = fs::canonicalize(root.join(&evidence.stored_path))?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(invalid("Evidence không nằm trong snapshot"));
        }
        Ok((path, evidence))
    }
    /// Imports one completed invocation/suite. Every import creates a distinct,
    /// immutable snapshot, even when its source directory or case names repeat.
    pub fn import_run(&self, project: &str, name: &str, input: &Path) -> io::Result<RunSummary> {
        let project = self.project(project)?;
        let source_root = fs::canonicalize(&project.source_dir)?;
        let input = fs::canonicalize(input)?;
        if !input.is_dir() || !input.starts_with(&source_root) {
            return Err(invalid(
                "Lượt chạy phải nằm trong thư mục nguồn của project",
            ));
        }
        if input.starts_with(&self.root) || self.root.starts_with(&input) {
            return Err(invalid("Nguồn import không được chồng lên kho history"));
        }
        self.import_tree(&project, name, &input, None, Vec::new())
    }

    pub fn ensure_auto_project(&self, key: &str, name: &str) -> io::Result<Project> {
        let marker = format!("auto:{key}");
        if marker.chars().count() > 200 || key.contains(['/', '\\', '\0']) {
            return Err(invalid("Nhóm history không hợp lệ"));
        }
        for project in self.projects()? {
            if project.source_dir == marker {
                return Ok(project);
            }
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            name: label(name)?,
            source_dir: marker,
            created_at: now(),
            origin: "auto".into(),
        };
        let stage = Staging(self.root.join(format!(".project-{}", project.id)));
        fs::create_dir(&stage.0)?;
        fs::create_dir(stage.0.join("runs"))?;
        write_json(&stage.0.join("project.json"), &project)?;
        fs::rename(&stage.0, self.project_dir(&project.id)?)?;
        Ok(project)
    }

    /// Copy a completed CLI/QC case into history. Failures here must not change the
    /// flow's PASS/FAIL; callers log the error and keep the original report.
    pub fn ingest_flow(&self, input: FlowIngest) -> io::Result<RunSummary> {
        let case_dir = fs::canonicalize(&input.case_dir)?;
        if !case_dir.is_dir()
            || case_dir.starts_with(&self.root)
            || self.root.starts_with(&case_dir)
        {
            return Err(invalid("Nguồn history không được chồng lên kho report"));
        }
        let inbox = inbox_dir(&self.root);
        fs::create_dir_all(&inbox)?;
        let inbox = fs::canonicalize(&inbox)?;
        let snapshot = if case_dir.starts_with(&inbox) {
            case_dir
        } else {
            let snapshot = inbox.join(Uuid::new_v4().to_string());
            copy_tree(&case_dir, &snapshot)?;
            snapshot
        };
        write_json(
            &snapshot.join("manifest.json"),
            &InboxManifest {
                source: Some(input.source.clone()),
                name: Some(input.name.clone()),
                app_id: input.app_id.clone(),
                url: input.url.clone(),
            },
        )?;
        let summary = self.ingest_snapshot(&snapshot, Some(&input))?;
        write_json(
            &snapshot.join("published.json"),
            &serde_json::json!({
                "projectId": summary.project_id,
                "runId": summary.id,
            }),
        )?;
        Ok(summary)
    }

    pub fn publish_inbox(&self) -> io::Result<Vec<RunSummary>> {
        let inbox = inbox_dir(&self.root);
        if !inbox.is_dir() {
            return Ok(Vec::new());
        }
        let mut published = Vec::new();
        for entry in fs::read_dir(&inbox)? {
            let path = entry?.path();
            if !path.is_dir() || path.join("published.json").is_file() {
                continue;
            }
            match self.ingest_snapshot(&path, None) {
                Ok(summary) => {
                    write_json(
                        &path.join("published.json"),
                        &serde_json::json!({
                            "projectId": summary.project_id,
                            "runId": summary.id,
                        }),
                    )?;
                    published.push(summary);
                }
                Err(_) => continue,
            }
        }
        Ok(published)
    }

    fn ingest_snapshot(
        &self,
        snapshot: &Path,
        input: Option<&FlowIngest>,
    ) -> io::Result<RunSummary> {
        let snapshot = fs::canonicalize(snapshot)?;
        if !snapshot.is_dir()
            || snapshot.starts_with(&self.root)
            || self.root.starts_with(&snapshot)
        {
            return Err(invalid("Nguồn history không được chồng lên kho report"));
        }
        let manifest = read_json::<InboxManifest>(&snapshot.join("manifest.json")).ok();
        let yaml = fs::read_to_string(snapshot.join("scenario.yaml")).ok();
        let mut reports = Vec::new();
        walk(&snapshot, &mut reports, 0)?;
        let report_path = reports
            .iter()
            .find(|path| path.file_name().is_some_and(|name| name == "report.json"))
            .ok_or_else(|| invalid("Cần report.json để ghi history"))?;
        let report: CaseReport = read_json(report_path)?;
        let app_id = input
            .and_then(|value| value.app_id.as_deref())
            .or_else(|| manifest.as_ref().and_then(|value| value.app_id.as_deref()));
        let url = input
            .and_then(|value| value.url.as_deref())
            .or_else(|| manifest.as_ref().and_then(|value| value.url.as_deref()));
        let groups = classify(&report, app_id, url, yaml.as_deref());
        let primary = groups
            .first()
            .cloned()
            .unwrap_or_else(crate::targets::unclassified);
        let project = self.ensure_auto_project(&primary.key, &primary.name)?;
        let name = input
            .map(|value| value.name.clone())
            .or_else(|| manifest.as_ref().and_then(|value| value.name.clone()))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| report.name.clone());
        let source = input
            .map(|value| value.source.clone())
            .or_else(|| manifest.and_then(|value| value.source))
            .filter(|value| !value.trim().is_empty());
        let summary = self.import_tree(
            &project,
            &name,
            &snapshot,
            source,
            groups.iter().map(|group| group.key.clone()).collect(),
        )?;
        let stored = self.run(&summary.project_id, &summary.id)?;
        for group in groups.iter().skip(1) {
            let extra = self.ensure_auto_project(&group.key, &group.name)?;
            if extra.id != stored.project_id {
                self.link_run(&extra, &stored)?;
            }
        }
        Ok(summary)
    }

    fn link_run(&self, project: &Project, run: &Run) -> io::Result<()> {
        let dest = self.run_dir(&project.id, &run.id)?;
        if dest.join("link.json").is_file() || dest.join("run.json").is_file() {
            return Ok(());
        }
        if dest.exists() {
            return Err(invalid("Thư mục run alias đã tồn tại"));
        }
        let stage = Staging(
            self.root
                .join(format!(".run-link-{}-{}", project.id, run.id)),
        );
        fs::create_dir(&stage.0)?;
        let mut summary = run.summary();
        summary.project_id = project.id.clone();
        write_json(&stage.0.join("summary.json"), &summary)?;
        write_json(
            &stage.0.join("link.json"),
            &RunLink {
                project_id: run.project_id.clone(),
                run_id: run.id.clone(),
            },
        )?;
        fs::rename(&stage.0, dest)?;
        Ok(())
    }

    fn import_tree(
        &self,
        project: &Project,
        name: &str,
        input: &Path,
        source: Option<String>,
        targets: Vec<String>,
    ) -> io::Result<RunSummary> {
        let name = label(name)?;
        let mut source_files = Vec::new();
        walk(input, &mut source_files, 0)?;
        let mut reports = source_files
            .iter()
            .filter(|path| path.file_name().is_some_and(|name| name == "report.json"))
            .cloned()
            .collect::<Vec<_>>();
        reports.sort();
        if reports.is_empty() || reports.len() > MAX_CASES {
            return Err(invalid("Cần từ 1 đến 5000 report.json trong một lượt chạy"));
        }
        let id = Uuid::new_v4().to_string();
        let stage = Staging(self.root.join(format!(".run-{id}")));
        fs::create_dir(&stage.0)?;
        let mut cases = Vec::new();
        let mut counts = Counts::default();
        let mut started = u64::MAX;
        let mut finished = 0;
        let mut warnings = Vec::new();
        for (case_id, path) in reports.iter().enumerate() {
            let report: CaseReport = read_json(path)?;
            if report.name.trim().is_empty()
                || !["PASS", "PARTIAL", "FAIL", "DRY_RUN", "NOT RUN"]
                    .contains(&report.status.as_str())
            {
                return Err(invalid(format!(
                    "Tên/trạng thái case không hợp lệ: {}",
                    path.display()
                )));
            }
            let modified = millis(fs::metadata(path)?.modified()?);
            started = started.min(modified);
            finished = finished.max(modified);
            match report.effective_status() {
                "PASS" => counts.passed += 1,
                "PARTIAL" => counts.partial += 1,
                "FAIL" => counts.failed += 1,
                _ => counts.not_run += 1,
            }
            counts.total += 1;
            let case_dir = path.parent().ok_or_else(|| invalid("Thiếu thư mục case"))?;
            let mut attachments = source_files
                .iter()
                .filter(|f| f.starts_with(case_dir))
                .cloned()
                .collect::<Vec<_>>();
            if let Some(parent) = case_dir.parent().filter(|parent| parent.starts_with(input)) {
                for entry in fs::read_dir(parent)? {
                    let entry = entry?;
                    if entry.file_type()?.is_file() && evidence_kind(&entry.path()).is_some() {
                        attachments.push(entry.path());
                    }
                    if entry.file_type()?.is_dir() && entry.file_name() == "native-artifacts" {
                        walk(&entry.path(), &mut attachments, 0)?;
                    }
                }
            }
            attachments.sort();
            attachments.dedup();
            let mut files = Vec::new();
            for source_file in attachments {
                let Some(kind) = evidence_kind(&source_file) else {
                    continue;
                };
                let canonical = fs::canonicalize(&source_file)?;
                if !canonical.starts_with(input)
                    || fs::symlink_metadata(&source_file)?.file_type().is_symlink()
                {
                    return Err(invalid("Evidence vượt ra ngoài nguồn import"));
                }
                let relative = source_file
                    .strip_prefix(case_dir)
                    .or_else(|_| source_file.strip_prefix(case_dir.parent().unwrap_or(case_dir)))
                    .map_err(io::Error::other)?;
                let display = relative.to_string_lossy().replace('\\', "/");
                if display == "published.json" {
                    continue;
                }
                let extension = source_file
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("txt")
                    .to_ascii_lowercase();
                let stored = format!("files/{case_id}/{}.{}", files.len(), extension);
                let destination = stage.0.join(&stored);
                fs::create_dir_all(
                    destination
                        .parent()
                        .ok_or_else(|| invalid("Thiếu thư mục evidence"))?,
                )?;
                fs::copy(&canonical, &destination)?;
                files.push(Evidence {
                    id: files.len(),
                    name: display,
                    kind: kind.into(),
                    bytes: fs::metadata(&destination)?.len(),
                    stored_path: stored,
                });
            }
            if !files.iter().any(|f| f.kind == "video") {
                warnings.push(format!(
                    "{}: không có video trong nguồn import",
                    report.name
                ));
            }
            cases.push(Case {
                id: case_id,
                duration_seconds: duration(input, path),
                report,
                files,
            });
        }
        let run = Run {
            schema_version: 1,
            id,
            project_id: project.id.clone(),
            name,
            imported_at: now(),
            first_report_at: started,
            last_report_at: finished,
            counts,
            cases,
            warnings,
            targets,
            source,
        };
        write_json(&stage.0.join("run.json"), &run)?;
        let summary = run.summary();
        write_json(&stage.0.join("summary.json"), &summary)?;
        fs::rename(&stage.0, self.run_dir(&run.project_id, &run.id)?)?;
        Ok(summary)
    }
}

fn walk(root: &Path, files: &mut Vec<PathBuf>, depth: usize) -> io::Result<()> {
    if depth > 32 || files.len() > MAX_FILES {
        return Err(invalid("Nguồn evidence quá sâu hoặc có quá nhiều tệp"));
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !entry.file_name().to_string_lossy().starts_with('.') {
                walk(&entry.path(), files, depth + 1)?;
            }
        } else if kind.is_file() && evidence_kind(&entry.path()).is_some() {
            if files.len() >= MAX_FILES {
                return Err(invalid("Nguồn evidence có quá nhiều tệp"));
            }
            files.push(entry.path());
        }
    }
    Ok(())
}
fn evidence_kind(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "webp" => Some("image"),
        "mp4" | "mov" | "webm" => Some("video"),
        "log" | "txt" | "jsonl" => Some("log"),
        "json" | "xml" | "yaml" | "yml" => Some("data"),
        _ => None,
    }
}
fn duration(input: &Path, report: &Path) -> Option<f64> {
    let summary: serde_json::Value = read_json(&input.join("summary.json")).ok()?;
    let case_name = report.parent()?.parent()?.file_name()?.to_str()?;
    summary
        .as_array()?
        .iter()
        .find(|value| value["case"].as_str() == Some(case_name))?
        .get("elapsed_seconds")?
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.)
}

pub fn default_reports_dir() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEVICE_BOT_REPORT_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?;
    let flavor = if cfg!(debug_assertions) {
        ".device-bot-dev"
    } else {
        ".device-bot"
    };
    Ok(PathBuf::from(home).join(flavor).join("reports"))
}

fn inbox_dir(root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("DEVICE_BOT_RUNS_DIR") {
        return PathBuf::from(path);
    }
    root.parent().unwrap_or(root).join("runs")
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let to = dst.join(entry.file_name());
        if kind.is_dir() {
            if !entry.file_name().to_string_lossy().starts_with('.') {
                copy_tree(&entry.path(), &to)?;
            }
        } else if kind.is_file() {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

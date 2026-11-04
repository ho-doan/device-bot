use crate::model::{Case, CaseReport, Counts, Evidence, Project, Run, RunSummary};
use serde::{Serialize, de::DeserializeOwned};
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
        Ok(Self {
            root: fs::canonicalize(root)?,
        })
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
    pub fn run(&self, project: &str, run: &str) -> io::Result<Run> {
        read_json(&self.run_dir(project, run)?.join("run.json"))
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
        let root = fs::canonicalize(self.run_dir(project, run)?)?;
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
        let name = label(name)?;
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
        let mut source_files = Vec::new();
        walk(&input, &mut source_files, 0)?;
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
            // robot-flow wrappers keep process/setup logs immediately above the
            // case directory. Never follow summary.json absolute report paths.
            if let Some(parent) = case_dir
                .parent()
                .filter(|parent| parent.starts_with(&input))
            {
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
            for source in attachments {
                let Some(kind) = evidence_kind(&source) else {
                    continue;
                };
                let canonical = fs::canonicalize(&source)?;
                if !canonical.starts_with(&input)
                    || fs::symlink_metadata(&source)?.file_type().is_symlink()
                {
                    return Err(invalid("Evidence vượt ra ngoài nguồn import"));
                }
                let relative = source
                    .strip_prefix(case_dir)
                    .or_else(|_| source.strip_prefix(case_dir.parent().unwrap_or(case_dir)))
                    .map_err(io::Error::other)?;
                let display = relative.to_string_lossy().replace('\\', "/");
                let extension = source
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
                duration_seconds: duration(&input, path),
                report,
                files,
            });
        }
        let run = Run {
            schema_version: 1,
            id,
            project_id: project.id,
            name,
            imported_at: now(),
            first_report_at: started,
            last_report_at: finished,
            counts,
            cases,
            warnings,
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
        "json" | "xml" => Some("data"),
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

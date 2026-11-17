//! Project manifests and orchestration; flows still execute through robot-flow/MCP.
pub mod plan;
mod recording;
mod report;
mod run;
pub mod session;
pub mod windows_fixture;

pub use report::ensure_report;
pub use run::run;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const MANIFEST: &str = "project.yaml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    kind: String,
    name: String,
    #[serde(default)]
    env_file: Option<PathBuf>,
    flows: Vec<plan::FlowEntry>,
    #[serde(default)]
    hosts: Vec<plan::Host>,
    #[serde(default)]
    resources: Vec<plan::Resource>,
    #[serde(default)]
    actors: Vec<plan::Actor>,
    #[serde(default)]
    windows_fixture: Option<String>,
    #[serde(default)]
    recording: Option<recording::RecordingConfig>,
}

pub struct Project {
    pub name: String,
    pub root: PathBuf,
    pub flows: Vec<PathBuf>,
    pub plan: plan::ProjectPlan,
    pub(crate) recording: Option<recording::RecordingConfig>,
    environment: BTreeMap<String, String>,
    windows_fixture: Option<String>,
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project")
            .field("name", &self.name)
            .field("root", &self.root)
            .field("flows", &self.flows)
            .field("plan", &self.plan)
            .field("recording", &self.recording)
            .field("environment", &"[redacted]")
            .finish_non_exhaustive()
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

impl Project {
    pub fn load(path: &Path) -> io::Result<Self> {
        let manifest = fs::canonicalize(if path.is_dir() {
            path.join(MANIFEST)
        } else {
            path.to_owned()
        })?;
        let root = manifest.parent().unwrap().to_owned();
        let model: Manifest = serde_yaml::from_str(&fs::read_to_string(&manifest)?)
            .map_err(|error| invalid(format!("invalid project manifest: {error}")))?;
        if model.version != 1
            || model.kind != "project"
            || model.name.trim().is_empty()
            || model.flows.is_empty()
        {
            return Err(invalid(
                "project requires version: 1, kind: project, a name and at least one flow",
            ));
        }
        let environment = if let Some(env_file) = model.env_file {
            let path = root.join(env_file);
            let entries = dotenvy::from_path_iter(&path)
                .map_err(|_| invalid(format!("cannot read project env file {}", path.display())))?;
            let mut environment = BTreeMap::new();
            for entry in entries {
                // dotenv parse errors can contain secret values; never display them.
                let (key, value) = entry.map_err(|_| invalid("invalid project env file syntax"))?;
                if std::env::var_os(&key).is_none() {
                    environment.insert(key, value);
                }
            }
            environment
        } else {
            BTreeMap::new()
        };
        let plan = plan::ProjectPlan::build(
            &root,
            model.hosts,
            model.resources,
            model.actors,
            model.flows,
        )?;
        recording::validate_config(model.recording.as_ref(), &plan)?;
        let flows = plan.flows.iter().map(|flow| flow.path.clone()).collect();
        Ok(Self {
            name: model.name,
            root,
            flows,
            plan,
            recording: model.recording,
            environment,
            windows_fixture: model.windows_fixture,
        })
    }

    pub(crate) fn environment_value(&self, key: &str) -> Option<String> {
        std::env::var(key)
            .ok()
            .or_else(|| self.environment.get(key).cloned())
    }

    /// Snapshot the effective environment for child sessions and compilation.
    pub fn environment(&self) -> BTreeMap<String, String> {
        let mut values = self.environment.clone();
        values.extend(
            std::env::vars_os().filter_map(|(key, value)| {
                Some((key.into_string().ok()?, value.into_string().ok()?))
            }),
        );
        values
    }

    pub fn resolve(&self, value: &str) -> io::Result<String> {
        let mut resolved = String::new();
        let mut rest = value;
        while let Some((prefix, reference)) = rest.split_once("${env:") {
            resolved.push_str(prefix);
            let (name, suffix) = reference
                .split_once('}')
                .ok_or_else(|| invalid("unclosed project env reference"))?;
            resolved.push_str(
                &self.environment_value(name).ok_or_else(|| {
                    invalid(format!("missing project environment variable {name}"))
                })?,
            );
            rest = suffix;
        }
        resolved.push_str(rest);
        Ok(resolved)
    }
}

/// Find the project positional argument while retaining the flow CLI's options.
/// A plain scenario remains on the existing robot-flow path.
pub fn invocation(arguments: &[String]) -> io::Result<Option<(Project, Vec<String>)>> {
    let values = [
        "--profiles",
        "--target",
        "--device",
        "--app-id",
        "--server",
        "--server-arg",
        "--mcp-url",
        "--artifacts",
        "--confirm",
        "--rpc-timeout",
        "--case-timeout",
    ];
    let mut paths = Vec::new();
    let mut i = 0;
    while i < arguments.len() {
        if values.contains(&arguments[i].as_str()) {
            i += 2;
        } else {
            if !arguments[i].starts_with('-') {
                paths.push(i);
            }
            i += 1;
        }
    }
    let mut project = None;
    for &index in &paths {
        let path = Path::new(&arguments[index]);
        let is_project = path.is_dir()
            || fs::read_to_string(path)
                .ok()
                .and_then(|source| serde_yaml::from_str::<serde_yaml::Value>(&source).ok())
                .is_some_and(|yaml| {
                    yaml.get("kind").and_then(|kind| kind.as_str()) == Some("project")
                });
        if is_project {
            if paths.len() != 1 {
                return Err(invalid(
                    "run one project at a time; do not mix it with scenario paths",
                ));
            }
            let mut forwarded = arguments.to_vec();
            forwarded.remove(index);
            project = Some((Project::load(path)?, forwarded));
        }
    }
    Ok(project)
}

/// Initialize a new or empty directory without overwriting existing content.
pub fn init(directory: &Path) -> io::Result<PathBuf> {
    if directory.exists() && fs::read_dir(directory)?.next().is_some() {
        return Err(invalid(
            "project destination must be a new or empty directory",
        ));
    }
    fs::create_dir_all(directory.join("flows"))?;
    for (path, content) in [
        (
            MANIFEST,
            "version: 1\nkind: project\nname: my-project\nenv_file: .env\nflows:\n  - flows/smoke.yaml\n",
        ),
        (
            ".env",
            "# Process environment overrides these project defaults.\nDEVICE_BOT_IOS_AGENT_BIND=127.0.0.1:0\nDEVICE_BOT_IOS_SIM_AGENT_BIND=127.0.0.1:0\n",
        ),
        (".gitignore", ".env\nartifacts/\n"),
        (
            "flows/smoke.yaml",
            "version: 2\nname: project_smoke\ndescription: Verify the project runner and MCP handshake without device actions.\nsteps:\n  - wait: {ms: 0}\n",
        ),
        (
            "README.md",
            "# Device Bot project\n\nRun `device-bot run . --dry-run` to compile, then `device-bot run .` for the MCP smoke.\nThe smoke checks orchestration only; replace/add YAML flows to test your app.\nPaths and artifacts resolve from this directory. Keep secrets in .env, not YAML.\n",
        ),
    ] {
        use std::io::Write;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(path))?
            .write_all(content.as_bytes())?;
    }
    fs::canonicalize(directory.join(MANIFEST))
}

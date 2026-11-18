//! Project manifests and orchestration; flows still execute through robot-flow/MCP.
mod artifact;
mod hook;
pub mod plan;
mod platform;
mod preparation;
mod recording;
mod report;
mod run;
pub mod session;
mod web_artifact;
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
    #[serde(default)]
    env_json: Option<JsonEnvironment>,
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
    #[serde(default)]
    platforms: BTreeMap<String, platform::Platform>,
    #[serde(default)]
    before_each: Option<hook::Hook>,
    #[serde(default)]
    before_each_flow: Option<PathBuf>,
    #[serde(default)]
    before_all_flow: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonEnvironment {
    path: PathBuf,
    prefixes: Vec<String>,
}

impl JsonEnvironment {
    fn load(&self, root: &Path, environment: &mut BTreeMap<String, String>) -> io::Result<()> {
        if self.prefixes.is_empty()
            || self.prefixes.iter().any(|prefix| {
                prefix.is_empty()
                    || !prefix.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
            })
        {
            return Err(invalid(
                "project JSON environment requires nonempty uppercase prefixes",
            ));
        }
        let input = fs::read(root.join(&self.path))
            .map_err(|_| invalid("cannot read project JSON environment file"))?;
        let input = input.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&input);
        let data: serde_json::Value = serde_json::from_slice(input)
            .map_err(|_| invalid("invalid project JSON environment file"))?;
        let object = data
            .as_object()
            .ok_or_else(|| invalid("project JSON environment must be an object"))?;
        for (key, value) in object {
            let Some(value) = value.as_str() else {
                continue;
            };
            if self.prefixes.iter().any(|prefix| key.starts_with(prefix))
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                && std::env::var_os(key).is_none()
            {
                if value.contains('\0') {
                    return Err(invalid(
                        "project JSON environment value contains a null byte",
                    ));
                }
                environment
                    .entry(key.clone())
                    .or_insert_with(|| value.to_owned());
            }
        }
        Ok(())
    }
}

pub struct Project {
    pub name: String,
    pub root: PathBuf,
    pub flows: Vec<PathBuf>,
    pub plan: plan::ProjectPlan,
    pub(crate) recording: Option<recording::RecordingConfig>,
    environment: BTreeMap<String, String>,
    windows_fixture: Option<String>,
    platforms: BTreeMap<String, platform::Platform>,
    before_each: Option<hook::Hook>,
    before_each_flow: Option<PathBuf>,
    before_all_flow: Option<PathBuf>,
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

pub(crate) fn fixture_flow_path(
    root: &Path,
    plan: &plan::ProjectPlan,
    path: &Path,
) -> io::Result<PathBuf> {
    let path = root.join(path);
    if !plan.is_legacy() || !path.is_file() {
        return Err(invalid(
            "fixture setup requires a readable flow in a path-only project",
        ));
    }
    let setup_path = fs::canonicalize(&path)?;
    if plan
        .flows
        .iter()
        .any(|flow| fs::canonicalize(&flow.path).ok().as_ref() == Some(&setup_path))
    {
        return Err(invalid(
            "fixture setup flow must not also be a project case",
        ));
    }
    Ok(path)
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
        let mut environment = if let Some(env_file) = model.env_file {
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
        if let Some(json) = &model.env_json {
            json.load(&root, &mut environment)?;
        }
        let plan = plan::ProjectPlan::build(
            &root,
            model.hosts,
            model.resources,
            model.actors,
            model.flows,
        )?;
        platform::validate(&model.platforms, &plan, &root, model.before_each.is_some())?;
        if let Some(hook) = &model.before_each {
            hook.validate(&plan)?;
        }
        if model.before_each.is_some() && model.before_each_flow.is_some() {
            return Err(invalid(
                "before_each and before_each_flow cannot be combined",
            ));
        }
        let before_each_flow = model
            .before_each_flow
            .as_deref()
            .map(|path| fixture_flow_path(&root, &plan, path))
            .transpose()?;
        let before_all_flow = model
            .before_all_flow
            .as_deref()
            .map(|path| fixture_flow_path(&root, &plan, path))
            .transpose()?;
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
            platforms: model.platforms,
            before_each: model.before_each,
            before_each_flow,
            before_all_flow,
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
        self.resolve_with(value, &BTreeMap::new())
    }

    pub(crate) fn resolve_with(
        &self,
        value: &str,
        overrides: &BTreeMap<String, String>,
    ) -> io::Result<String> {
        let mut resolved = String::new();
        let mut rest = value;
        while let Some((prefix, reference)) = rest.split_once("${env:") {
            resolved.push_str(prefix);
            let (name, suffix) = reference
                .split_once('}')
                .ok_or_else(|| invalid("unclosed project env reference"))?;
            resolved.push_str(
                &overrides
                    .get(name)
                    .cloned()
                    .or_else(|| self.environment_value(name))
                    .ok_or_else(|| {
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
        "--platform",
        "--artifact",
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

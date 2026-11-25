//! Declarative project plan. This module validates configuration; it does not
//! acquire resources or claim runtime capability.
use crate::invalid;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostBinding {
    Local {},
    Mcp {
        endpoint: String,
        #[serde(default)]
        allow_remote: bool,
        #[serde(default)]
        token_env: Option<String>,
    },
}
impl std::fmt::Debug for HostBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local {} => f.write_str("Local"),
            Self::Mcp { .. } => f.write_str("Mcp { endpoint: [redacted] }"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub name: String,
    pub platform: HostPlatform,
    pub binding: HostBinding,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Android,
    IosSimulator,
    IosReal,
    Chrome,
    Windows,
    Macos,
}

/// How a resource on another execution host appears on a display host.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteDisplay {
    Mirror,
    RemoteDesktop,
}

/// Declares intent only; the project runtime must verify ownership and capability.
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Preparation {
    Attach {},
    Launch {
        #[serde(default)]
        app_id: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        boot: bool,
    },
}
impl std::fmt::Debug for Preparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attach {} => f.write_str("Attach"),
            Self::Launch { .. } => f.write_str("Launch { values: [redacted] }"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Discovery,
    Launch,
    Input,
    Screenshot,
    Elements,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayBinding {
    pub host: String,
    #[serde(default)]
    pub via: Option<RemoteDisplay>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebProxySpec {
    pub upstream: String,
    #[serde(default)]
    pub passthrough: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub name: String,
    pub kind: ResourceKind,
    /// Host where the resource's MCP driver executes.
    pub host: String,
    /// Host displaying the resource; does not imply a recorder exists.
    pub display: DisplayBinding,
    /// Optional stable device/app selector interpreted by the future runtime.
    #[serde(default)]
    pub selector: Option<String>,
    pub prepare: Preparation,
    pub timeout_ms: u64,
    #[serde(default)]
    pub requires: Vec<Capability>,
    #[serde(default)]
    pub web_proxy: Option<WebProxySpec>,
}

/// A named browser actor owns one dedicated Chrome resource/session.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub name: String,
    pub resource: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowBinding {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedFlow {
    pub path: PathBuf,
    #[serde(default)]
    pub profiles: Option<PathBuf>,
    pub bindings: Vec<FlowBinding>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FlowEntry {
    Legacy(PathBuf),
    Bound(NamedFlow),
}

#[derive(Clone, Debug)]
pub struct PlannedFlow {
    pub path: PathBuf,
    pub profiles: Option<PathBuf>,
    /// Empty only for a legacy flow; a bound entry always has bindings.
    pub bindings: Vec<FlowBinding>,
}

#[derive(Clone, Debug)]
pub struct ProjectPlan {
    pub hosts: Vec<Host>,
    pub resources: Vec<Resource>,
    pub actors: Vec<Actor>,
    pub flows: Vec<PlannedFlow>,
}

impl ProjectPlan {
    pub fn is_legacy(&self) -> bool {
        self.hosts.is_empty()
            && self.resources.is_empty()
            && self.flows.iter().all(|f| f.bindings.is_empty())
    }

    pub(crate) fn build(
        root: &Path,
        hosts: Vec<Host>,
        resources: Vec<Resource>,
        actors: Vec<Actor>,
        entries: Vec<FlowEntry>,
    ) -> io::Result<Self> {
        let mut host_names = HashSet::new();
        for host in &hosts {
            name(&host.name, "host")?;
            if !host_names.insert(host.name.to_ascii_lowercase()) {
                return Err(invalid("duplicate host name"));
            }
            if let HostBinding::Mcp { endpoint, .. } = &host.binding {
                if endpoint.trim().is_empty() {
                    return Err(invalid("MCP endpoint cannot be empty"));
                }
            }
        }
        let mut resource_names = HashSet::new();
        let mut resource_path_names = HashSet::new();
        for resource in &resources {
            name(&resource.name, "resource")?;
            if !resource_names.insert(resource.name.as_str())
                || !resource_path_names.insert(resource.name.to_ascii_lowercase())
            {
                return Err(invalid("duplicate resource name"));
            }
            let host = hosts
                .iter()
                .find(|h| h.name == resource.host)
                .ok_or_else(|| invalid("resource references unknown execution host"))?;
            let display = hosts
                .iter()
                .find(|h| h.name == resource.display.host)
                .ok_or_else(|| invalid("resource references unknown display host"))?;
            if display.platform == HostPlatform::Linux {
                return Err(invalid("Linux desktop display is not supported"));
            }
            if resource.host != resource.display.host && resource.display.via.is_none() {
                return Err(invalid(
                    "remote display requires an explicit mirror or remote_desktop",
                ));
            }
            if resource.host == resource.display.host && resource.display.via.is_some() {
                return Err(invalid(
                    "local display cannot declare a remote display method",
                ));
            }
            if matches!(
                (resource.kind, host.platform),
                (
                    ResourceKind::IosSimulator | ResourceKind::IosReal | ResourceKind::Macos,
                    HostPlatform::Windows | HostPlatform::Linux
                ) | (
                    ResourceKind::Windows,
                    HostPlatform::Macos | HostPlatform::Linux
                )
            ) {
                return Err(invalid(
                    "resource kind is incompatible with execution host platform",
                ));
            }
            if resource
                .selector
                .as_ref()
                .is_some_and(|s| s.trim().is_empty())
            {
                return Err(invalid("resource selector cannot be empty"));
            }
            if resource.timeout_ms == 0 {
                return Err(invalid("resource timeout_ms must be positive"));
            }
            if resource.web_proxy.is_some()
                && (resource.kind != ResourceKind::Chrome
                    || !matches!(resource.prepare, Preparation::Launch { .. }))
            {
                return Err(invalid(
                    "web_proxy requires a project-owned launched Chrome resource",
                ));
            }
            let mut requirements = HashSet::new();
            if resource
                .requires
                .iter()
                .any(|capability| !requirements.insert(capability))
            {
                return Err(invalid("duplicate resource capability"));
            }
            if let Preparation::Launch { app_id, url, boot } = &resource.prepare {
                let valid =
                    |value: &Option<String>| value.as_ref().is_some_and(|s| !s.trim().is_empty());
                if resource.kind == ResourceKind::Chrome {
                    if !valid(url) || app_id.is_some() || *boot {
                        return Err(invalid(
                            "Chrome launch requires url and does not support app_id or boot",
                        ));
                    }
                } else if !valid(app_id)
                    || url.is_some()
                    || (*boot
                        && !matches!(
                            resource.kind,
                            ResourceKind::Android | ResourceKind::IosSimulator
                        ))
                {
                    return Err(invalid(
                        "native launch requires app_id; boot is only valid for Android or iOS Simulator",
                    ));
                }
            }
        }
        let mut actor_names = HashSet::new();
        let mut actor_resources = HashSet::new();
        for actor in &actors {
            name(&actor.name, "actor")?;
            if !actor_names.insert(actor.name.to_ascii_lowercase()) {
                return Err(invalid("duplicate actor name"));
            }
            if resource_path_names.contains(&actor.name.to_ascii_lowercase()) {
                return Err(invalid("actor name collides with resource name"));
            }
            if !actor_resources.insert(actor.resource.as_str()) {
                return Err(invalid("each actor must own a distinct Chrome resource"));
            }
            let resource = resources
                .iter()
                .find(|resource| resource.name == actor.resource)
                .ok_or_else(|| invalid("actor references unknown resource"))?;
            if resource.kind != ResourceKind::Chrome {
                return Err(invalid("actor resource must be Chrome"));
            }
            if !matches!(&resource.prepare, Preparation::Launch { .. }) {
                return Err(invalid(
                    "actor resource must use prepare: launch for isolated context",
                ));
            }
        }
        let mut flows = Vec::new();
        for entry in entries {
            let (path, profiles, bindings) = match entry {
                FlowEntry::Legacy(path) => (path, None, Vec::new()),
                FlowEntry::Bound(flow) => {
                    if flow.bindings.is_empty() {
                        return Err(invalid("bound flow requires at least one binding"));
                    }
                    let mut targets = HashSet::new();
                    for binding in &flow.bindings {
                        let resource_name = match (&binding.target, &binding.actor) {
                            (Some(target), None) => {
                                name(target, "flow target")?;
                                if !resource_names.contains(target.as_str()) {
                                    return Err(invalid("flow references unknown target"));
                                }
                                target.as_str()
                            }
                            (None, Some(actor_name)) => {
                                name(actor_name, "flow actor")?;
                                let actor =
                                    actors
                                        .iter()
                                        .find(|actor| actor.name == *actor_name)
                                        .ok_or_else(|| invalid("flow references unknown actor"))?;
                                actor.resource.as_str()
                            }
                            _ => {
                                return Err(invalid(
                                    "flow binding must specify exactly one of target or actor",
                                ));
                            }
                        };
                        if !targets.insert(resource_name) {
                            return Err(invalid("duplicate target binding in flow"));
                        }
                        if binding
                            .profile
                            .as_ref()
                            .is_some_and(|p| p.trim().is_empty())
                        {
                            return Err(invalid("flow profile cannot be empty"));
                        }
                    }
                    if flow
                        .bindings
                        .iter()
                        .any(|binding| binding.profile.is_some())
                        && flow.profiles.is_none()
                    {
                        return Err(invalid("flow profile requires a profiles file"));
                    }
                    let profiles = flow.profiles.map(|path| root.join(path));
                    if profiles.as_ref().is_some_and(|path| !path.is_file()) {
                        return Err(invalid("flow profiles file does not exist"));
                    }
                    (flow.path, profiles, flow.bindings)
                }
            };
            for path in flow_paths(&root.join(path))? {
                flows.push(PlannedFlow {
                    path,
                    profiles: profiles.clone(),
                    bindings: bindings.clone(),
                });
            }
        }
        Ok(Self {
            hosts,
            resources,
            actors,
            flows,
        })
    }
}

/// A directory selects its immediate YAML files in deterministic order.
fn flow_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let is_yaml = |path: &Path| {
        path.extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
    };
    if path.is_file() && is_yaml(path) {
        return Ok(vec![path.to_owned()]);
    }
    if path.is_dir() {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && is_yaml(&entry.path()) {
                paths.push(entry.path());
            }
        }
        paths.sort();
        if !paths.is_empty() {
            return Ok(paths);
        }
        return Err(invalid(format!(
            "project flow directory contains no YAML files: {}",
            path.display()
        )));
    }
    Err(invalid(format!(
        "project flow must be an existing YAML file or directory: {}",
        path.display()
    )))
}
fn name(value: &str, kind: &str) -> io::Result<()> {
    // Resource names become directory components, so enforce the same portable
    // identifier grammar on every platform, including Windows reserved names.
    let reserved = matches!(
        value.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if value.is_empty()
        || value.len() > 64
        || reserved
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(invalid(format!(
            "{kind} name must be 1..64 ASCII letters, digits, underscores or hyphens, start with a letter or digit, and not be a reserved device name"
        )));
    }
    Ok(())
}

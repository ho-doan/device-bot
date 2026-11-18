//! Platform-specific defaults for path-only project runs.
use crate::{Project, invalid, plan::ProjectPlan};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Platform {
    profiles: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    prepare: Option<crate::preparation::Preparation>,
    #[serde(default)]
    before_each_flow: Option<PathBuf>,
    #[serde(default)]
    before_all_flow: Option<PathBuf>,
}

pub(crate) fn validate(
    platforms: &BTreeMap<String, Platform>,
    plan: &ProjectPlan,
    root: &Path,
    has_command_hook: bool,
) -> io::Result<()> {
    if !platforms.is_empty() && !plan.is_legacy() {
        return Err(invalid(
            "platform defaults currently require a path-only project",
        ));
    }
    for (name, platform) in platforms {
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(invalid("invalid project platform name"));
        }
        if platform.profiles.trim().is_empty()
            || platform
                .target
                .as_ref()
                .is_some_and(|target| target.trim().is_empty())
        {
            return Err(invalid(
                "project platform requires a profiles file and nonempty target",
            ));
        }
        if let Some(preparation) = &platform.prepare {
            preparation.validate(platform.target.as_deref().unwrap_or(name))?;
        }
        if let Some(path) = &platform.before_each_flow {
            if has_command_hook {
                return Err(invalid(
                    "before_each and before_each_flow cannot be combined",
                ));
            }
            crate::fixture_flow_path(root, plan, path)?;
        }
        if let Some(path) = &platform.before_all_flow {
            crate::fixture_flow_path(root, plan, path)?;
        }
    }
    Ok(())
}

/// Translate project-only platform selection into existing flow runner options.
pub(crate) struct Selection {
    pub(crate) arguments: Vec<String>,
    pub(crate) platform: Option<String>,
    pub(crate) artifact: Option<String>,
    pub(crate) environment: BTreeMap<String, String>,
    pub(crate) file_preparation: Option<crate::artifact::FilePreparation>,
    pub(crate) file_argument_indexes: Vec<usize>,
    pub(crate) web_argument_indexes: Vec<usize>,
    pub(crate) before_each_flow: Option<PathBuf>,
    pub(crate) before_all_flow: Option<PathBuf>,
}

pub(crate) fn arguments(project: &Project, arguments: &[String]) -> io::Result<Selection> {
    let mut forwarded = Vec::with_capacity(arguments.len() + 8);
    let mut selected = None;
    let mut artifact = None;
    let mut flags = Vec::new();
    let mut i = 0;
    while i < arguments.len() {
        if arguments[i] == "--platform" {
            let value = arguments
                .get(i + 1)
                .ok_or_else(|| invalid("--platform requires a value"))?;
            if value.starts_with('-') || selected.replace(value.as_str()).is_some() {
                return Err(invalid("specify exactly one project --platform"));
            }
            i += 2;
        } else if arguments[i] == "--artifact" {
            let value = arguments
                .get(i + 1)
                .ok_or_else(|| invalid("--artifact requires a value"))?;
            if value.trim().is_empty()
                || value.starts_with('-')
                || artifact.replace(value.clone()).is_some()
            {
                return Err(invalid("specify exactly one nonempty project --artifact"));
            }
            i += 2;
        } else {
            forwarded.push(arguments[i].clone());
            let takes_value = matches!(
                arguments[i].as_str(),
                "--profiles"
                    | "--target"
                    | "--device"
                    | "--app-id"
                    | "--server"
                    | "--server-arg"
                    | "--mcp-url"
                    | "--artifacts"
                    | "--confirm"
                    | "--rpc-timeout"
                    | "--case-timeout"
            );
            if takes_value {
                flags.push(arguments[i].as_str());
                let value = arguments
                    .get(i + 1)
                    .ok_or_else(|| invalid(format!("{} requires a value", arguments[i])))?;
                forwarded.push(value.clone());
                i += 2;
            } else {
                i += 1;
            }
        }
    }
    let Some(selected) = selected else {
        if project.platforms.is_empty() {
            if artifact.is_some() && project.before_each.is_none() {
                return Err(invalid(
                    "--artifact requires a configured preparation adapter",
                ));
            }
            return Ok(Selection {
                arguments: forwarded,
                platform: None,
                artifact,
                environment: BTreeMap::new(),
                file_preparation: None,
                file_argument_indexes: Vec::new(),
                web_argument_indexes: Vec::new(),
                before_each_flow: project.before_each_flow.clone(),
                before_all_flow: project.before_all_flow.clone(),
            });
        }
        return Err(invalid("project requires --platform"));
    };
    let config = project
        .platforms
        .get(selected)
        .ok_or_else(|| invalid("unknown project platform"))?;
    if artifact.is_some() && config.prepare.is_none() && project.before_each.is_none() {
        return Err(invalid(
            "--artifact requires a configured preparation adapter",
        ));
    }
    let environment = match &config.prepare {
        Some(preparation) => preparation.prepare(project, artifact.as_deref())?,
        None => BTreeMap::new(),
    };
    let file_preparation = config
        .prepare
        .as_ref()
        .and_then(crate::preparation::Preparation::file_preparation);
    if flags.contains(&"--profiles") || flags.contains(&"--target") {
        return Err(invalid(
            "--profiles and --target cannot override a project platform",
        ));
    }
    let profile = project.root.join(project.resolve(&config.profiles)?);
    if !profile.is_file() {
        return Err(invalid(
            "selected project platform profiles file does not exist",
        ));
    }
    let profile = profile
        .to_str()
        .ok_or_else(|| invalid("platform profiles path is not UTF-8"))?;
    forwarded.extend([
        "--profiles".into(),
        profile.into(),
        "--target".into(),
        config.target.as_deref().unwrap_or(selected).into(),
    ]);
    let mut file_argument_indexes = Vec::new();
    let mut web_argument_indexes = Vec::new();
    for (flag, default) in [("--device", &config.device), ("--app-id", &config.app_id)] {
        if !flags.contains(&flag) {
            if let Some(value) = default {
                let uses_file = file_preparation.as_ref().is_some_and(|preparation| {
                    value.contains(&format!("${{env:{}}}", preparation.path_env))
                });
                let uses_web = file_preparation
                    .as_ref()
                    .and_then(|preparation| preparation.serve_web.as_ref())
                    .is_some_and(|web| value.contains(&format!("${{env:{}}}", web.base_url_env)));
                let value = project.resolve_with(value, &environment)?;
                if value.trim().is_empty() {
                    return Err(invalid(format!("project platform {flag} cannot be empty")));
                }
                if uses_file {
                    file_argument_indexes.push(forwarded.len() + 1);
                }
                if uses_web {
                    web_argument_indexes.push(forwarded.len() + 1);
                }
                forwarded.extend([flag.into(), value]);
            }
        }
    }
    Ok(Selection {
        arguments: forwarded,
        platform: Some(selected.to_owned()),
        artifact,
        environment,
        file_preparation,
        file_argument_indexes,
        web_argument_indexes,
        before_each_flow: config
            .before_each_flow
            .as_ref()
            .map(|path| project.root.join(path))
            .or_else(|| project.before_each_flow.clone()),
        before_all_flow: config
            .before_all_flow
            .as_ref()
            .map(|path| project.root.join(path))
            .or_else(|| project.before_all_flow.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("project-platform-{}", uuid::Uuid::new_v4()));
            crate::init(&root).unwrap();
            fs::write(root.join("profiles.yaml"), "version: 1\ntargets: {}\n").unwrap();
            fs::write(
                root.join(".env"),
                "PROJECT_PLATFORM_APP=com.example.app\nPROJECT_PLATFORM_DEVICE=device-123\n",
            )
            .unwrap();
            fs::write(
                root.join("project.yaml"),
                r#"version: 1
kind: project
name: example
env_file: .env
flows: [flows]
platforms:
  web:
    profiles: profiles.yaml
    target: chrome
    device: '${env:PROJECT_PLATFORM_DEVICE}'
    app_id: '${env:PROJECT_PLATFORM_APP}'
"#,
            )
            .unwrap();
            Self(root)
        }
        fn project(&self) -> Project {
            Project::load(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn selects_profile_and_resolves_scoped_defaults_without_global_mutation() {
        let f = Fixture::new();
        let project = f.project();
        let actual = arguments(&project, &args(&["--platform", "web", "--dry-run"]))
            .unwrap()
            .arguments;
        assert_eq!(
            actual,
            vec![
                "--dry-run",
                "--profiles",
                project.root.join("profiles.yaml").to_str().unwrap(),
                "--target",
                "chrome",
                "--device",
                "device-123",
                "--app-id",
                "com.example.app"
            ]
        );
        assert!(std::env::var_os("PROJECT_PLATFORM_DEVICE").is_none());
    }

    #[test]
    fn explicit_device_and_app_override_defaults_and_option_values_are_not_flags() {
        let f = Fixture::new();
        let actual = arguments(
            &f.project(),
            &args(&[
                "--platform",
                "web",
                "--device",
                "chosen",
                "--app-id",
                "custom",
                "--server-arg",
                "--platform",
            ]),
        )
        .unwrap()
        .arguments;
        assert_eq!(actual.iter().filter(|arg| *arg == "--device").count(), 1);
        assert_eq!(actual.iter().filter(|arg| *arg == "--app-id").count(), 1);
        assert!(
            actual
                .windows(2)
                .any(|pair| pair == ["--server-arg", "--platform"])
        );
    }

    #[test]
    fn invalid_selection_conflicts_and_missing_profile_fail_early() {
        let f = Fixture::new();
        let project = f.project();
        for values in [
            vec![],
            vec!["--platform"],
            vec!["--platform", "unknown"],
            vec!["--platform", "web", "--platform", "web"],
            vec!["--platform", "web", "--target", "android"],
        ] {
            assert!(arguments(&project, &args(&values)).is_err(), "{values:?}");
        }
        fs::remove_file(f.0.join("profiles.yaml")).unwrap();
        assert!(arguments(&project, &args(&["--platform", "web"])).is_err());
    }

    #[test]
    fn artifact_is_passed_only_to_a_declared_handler() {
        let f = Fixture::new();
        let input = args(&[
            "--platform",
            "web",
            "--artifact",
            "https://example.test/file?token=secret",
        ]);
        assert!(arguments(&f.project(), &input).is_err());
        let manifest = f.0.join("project.yaml");
        let mut content = fs::read_to_string(&manifest).unwrap();
        content.push_str("before_each: {command: prepare-app}\n");
        fs::write(manifest, content).unwrap();
        let selection = arguments(&f.project(), &input).unwrap();
        assert_eq!(
            selection.artifact.as_deref(),
            Some("https://example.test/file?token=secret")
        );
        assert!(
            !selection
                .arguments
                .iter()
                .any(|arg| arg.contains("secret") || arg == "--artifact")
        );
        let duplicate = args(&[
            "--platform",
            "web",
            "--artifact",
            "file",
            "--artifact",
            "other",
        ]);
        assert!(arguments(&f.project(), &duplicate).is_err());
    }

    #[test]
    fn built_in_web_url_preparation_scopes_the_selected_url_to_the_run() {
        let f = Fixture::new();
        let manifest = f.0.join("project.yaml");
        let content = fs::read_to_string(&manifest).unwrap().replace(
            "    target: chrome\n",
            "    target: chrome\n    prepare: {kind: web_url, base_url_env: PROJECT_TEST_WEB_URL}\n",
        ).replace("${env:PROJECT_PLATFORM_APP}", "${env:PROJECT_TEST_WEB_URL}");
        fs::write(manifest, content).unwrap();
        let project = f.project();
        let actual = arguments(
            &project,
            &args(&[
                "--platform",
                "web",
                "--artifact",
                "http://127.0.0.1:8080/app/#/",
            ]),
        )
        .unwrap();
        assert_eq!(
            actual.environment.get("PROJECT_TEST_WEB_URL").unwrap(),
            "http://127.0.0.1:8080/app"
        );
        assert!(std::env::var_os("PROJECT_TEST_WEB_URL").is_none());
        assert!(
            actual
                .arguments
                .windows(2)
                .any(|pair| pair == ["--app-id", "http://127.0.0.1:8080/app"])
        );
        for invalid in [
            "ftp://example.test/app",
            "https://user:password@example.test/",
            "https://example.test/?token=secret",
            "https://example.test/#/route",
            "app.zip",
        ] {
            let error = arguments(
                &project,
                &args(&["--platform", "web", "--artifact", invalid]),
            )
            .err()
            .expect("invalid input accepted")
            .to_string();
            assert!(!error.contains("password") && !error.contains("token=secret"));
        }
        assert!(arguments(&project, &args(&["--platform", "web"])).is_err());
    }

    #[test]
    fn artifact_file_supports_gitlab_auth_from_project_environment() {
        let f = Fixture::new();
        let manifest = f.0.join("project.yaml");
        let source = fs::read_to_string(&manifest).unwrap().replace(
            "    target: chrome\n",
            "    target: chrome\n    prepare:\n      kind: artifact_file\n      path_env: APP_FILE\n      file_name: build.zip\n      auth: {kind: gitlab_job_token, token_env: TEST_JOB_TOKEN, origin_env: TEST_GITLAB_ORIGIN}\n",
        );
        fs::write(manifest, source).unwrap();
        fs::write(
            f.0.join(".env"),
            "PROJECT_PLATFORM_APP=app\nPROJECT_PLATFORM_DEVICE=device\nTEST_JOB_TOKEN=secret\nTEST_GITLAB_ORIGIN=https://gitlab.example.test\n",
        )
        .unwrap();
        let project = f.project();
        let selection = arguments(
            &project,
            &args(&[
                "--platform",
                "web",
                "--artifact",
                "https://gitlab.example.test/api/artifact",
            ]),
        )
        .unwrap();
        assert!(selection.file_preparation.is_some());
        assert!(!format!("{project:?}").contains("secret"));
        assert!(!selection.arguments.iter().any(|arg| arg.contains("secret")));
    }
}

//! Built-in preparation adapters selected declaratively by a project platform.
use crate::{Project, invalid};
use serde::Deserialize;
use std::{collections::BTreeMap, io};
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Preparation {
    /// Run flows against an existing HTTP(S) web application.
    WebUrl { base_url_env: String },
    /// Stage a file or HTTP(S) download for use by a flow.
    ArtifactFile {
        path_env: String,
        #[serde(default)]
        sha256_env: Option<String>,
        #[serde(default = "default_artifact_name")]
        file_name: String,
        #[serde(default = "default_max_bytes")]
        max_bytes: u64,
        #[serde(default)]
        auth: Option<crate::artifact::DownloadAuth>,
        #[serde(default)]
        extract: Option<crate::artifact::Extraction>,
    },
}

fn default_max_bytes() -> u64 {
    2 * 1024 * 1024 * 1024
}
fn default_artifact_name() -> String {
    "artifact".into()
}

fn valid_variable(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || (index > 0 && byte.is_ascii_digit())
        })
        && !value.starts_with("DEVICE_BOT_")
        && !matches!(
            value,
            "PATH" | "HOME" | "LD_PRELOAD" | "DYLD_INSERT_LIBRARIES"
        )
}

fn valid_filename(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let windows_numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && matches!(stem.as_bytes()[3], b'1'..=b'9');
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !windows_numbered
}

impl Preparation {
    pub(crate) fn validate(&self, target: &str) -> io::Result<()> {
        match self {
            Self::WebUrl { base_url_env } => {
                if target != "chrome" || !valid_variable(base_url_env) {
                    return Err(invalid(
                        "web_url preparation requires target chrome and a valid application environment variable",
                    ));
                }
            }
            Self::ArtifactFile {
                path_env,
                sha256_env,
                file_name,
                max_bytes,
                auth,
                extract,
            } => {
                if !valid_variable(path_env)
                    || sha256_env
                        .as_ref()
                        .is_some_and(|name| !valid_variable(name) || name == path_env)
                    || !valid_filename(file_name)
                    || *max_bytes == 0
                    || *max_bytes > 10 * 1024 * 1024 * 1024
                    || auth.as_ref().is_some_and(|auth| match auth {
                        crate::artifact::DownloadAuth::GitlabJobToken {
                            token_env,
                            origin_env,
                        } => !valid_variable(token_env) || !valid_variable(origin_env),
                    })
                {
                    return Err(invalid(
                        "artifact_file requires valid environment names and a size limit up to 10 GiB",
                    ));
                }
                if let Some(extract) = extract {
                    extract.validate()?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn prepare(
        &self,
        project: &Project,
        artifact: Option<&str>,
    ) -> io::Result<BTreeMap<String, String>> {
        match self {
            Self::WebUrl { base_url_env } => {
                let value = artifact
                    .map(str::to_owned)
                    .or_else(|| project.environment_value(base_url_env))
                    .ok_or_else(|| {
                        invalid(format!("web_url requires --artifact or {base_url_env}"))
                    })?;
                let mut parsed = Url::parse(&value)
                    .map_err(|_| invalid("web_url requires an absolute HTTP(S) URL"))?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed
                        .fragment()
                        .is_some_and(|fragment| fragment != "/" && !fragment.is_empty())
                {
                    return Err(invalid(
                        "web_url requires an HTTP(S) URL without credentials, query or route fragment",
                    ));
                }
                parsed.set_fragment(None);
                let base = parsed.as_str().trim_end_matches('/').to_owned();
                Ok(BTreeMap::from([(base_url_env.clone(), base)]))
            }
            Self::ArtifactFile {
                path_env,
                sha256_env,
                file_name,
                max_bytes,
                auth,
                extract,
            } => {
                let source =
                    artifact.ok_or_else(|| invalid("artifact_file requires --artifact"))?;
                let config = crate::artifact::FilePreparation {
                    path_env: path_env.clone(),
                    sha256_env: sha256_env.clone(),
                    file_name: file_name.clone(),
                    max_bytes: *max_bytes,
                    auth: auth.clone(),
                    extract: extract.clone(),
                };
                config.validate_source(project, source)?;
                Ok(BTreeMap::from([(
                    path_env.clone(),
                    crate::artifact::DRY_RUN_PLACEHOLDER.into(),
                )]))
            }
        }
    }

    pub(crate) fn file_preparation(&self) -> Option<crate::artifact::FilePreparation> {
        match self {
            Self::ArtifactFile {
                path_env,
                sha256_env,
                file_name,
                max_bytes,
                auth,
                extract,
            } => Some(crate::artifact::FilePreparation {
                path_env: path_env.clone(),
                sha256_env: sha256_env.clone(),
                file_name: file_name.clone(),
                max_bytes: *max_bytes,
                auth: auth.clone(),
                extract: extract.clone(),
            }),
            Self::WebUrl { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::valid_filename;

    #[test]
    fn staged_file_names_are_portable_single_components() {
        for valid in ["app.apk", "build-42.zip", "_artifact"] {
            assert!(valid_filename(valid), "{valid}");
        }
        for invalid in [
            "",
            ".",
            "..",
            "../app.apk",
            "sub/app",
            "sub\\app",
            "C:app",
            "NUL.zip",
            "com1.apk",
            "lpt9",
            "trailing.",
        ] {
            assert!(!valid_filename(invalid), "{invalid}");
        }
    }
}

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
}

impl Preparation {
    pub(crate) fn validate(&self, target: &str) -> io::Result<()> {
        match self {
            Self::WebUrl { base_url_env } => {
                let valid = !base_url_env.is_empty()
                    && base_url_env.bytes().enumerate().all(|(index, byte)| {
                        byte == b'_'
                            || byte.is_ascii_uppercase()
                            || (index > 0 && byte.is_ascii_digit())
                    });
                if target != "chrome" || !valid || base_url_env.starts_with("DEVICE_BOT_") {
                    return Err(invalid(
                        "web_url preparation requires target chrome and a valid application environment variable",
                    ));
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
        }
    }
}

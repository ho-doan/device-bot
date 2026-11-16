use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum ScenarioPathError {
    NotFound(PathBuf),
    DraftRequiresOptIn(PathBuf),
}

impl std::fmt::Display for ScenarioPathError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(formatter, "scenario not found: {}", path.display()),
            Self::DraftRequiresOptIn(path) => write!(
                formatter,
                "{} is a draft; pass --include-draft to run it explicitly",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ScenarioPathError {}

/// Resolve a runnable scenario. Drafts are invisible unless a human opts in.
pub fn resolve_scenario(
    suite: &Path,
    name: &str,
    include_draft: bool,
) -> Result<PathBuf, ScenarioPathError> {
    let yaml = suite.join(format!("{name}.yaml"));
    if yaml.is_file() {
        return Ok(yaml);
    }

    let draft = suite.join(format!("{name}.yaml.draft"));
    if draft.is_file() {
        return if include_draft {
            Ok(draft)
        } else {
            Err(ScenarioPathError::DraftRequiresOptIn(draft))
        };
    }

    Err(ScenarioPathError::NotFound(yaml))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "automation-test-harness-{}-{:?}",
                std::process::id(),
                SystemTime::now()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn draft_is_not_runnable_without_explicit_opt_in() {
        let suite = TempDir::new();
        let draft = suite.0.join("candidate.yaml.draft");
        fs::write(&draft, "version: 2\n").unwrap();

        assert_eq!(
            resolve_scenario(&suite.0, "candidate", false),
            Err(ScenarioPathError::DraftRequiresOptIn(draft))
        );
    }

    #[test]
    fn explicit_opt_in_resolves_draft_without_renaming_it() {
        let suite = TempDir::new();
        let draft = suite.0.join("candidate.yaml.draft");
        fs::write(&draft, "version: 2\n").unwrap();

        assert_eq!(resolve_scenario(&suite.0, "candidate", true), Ok(draft));
    }

    #[test]
    fn promoted_yaml_takes_precedence_over_draft() {
        let suite = TempDir::new();
        let yaml = suite.0.join("candidate.yaml");
        fs::write(&yaml, "version: 2\n").unwrap();
        fs::write(suite.0.join("candidate.yaml.draft"), "version: 2\n").unwrap();

        assert_eq!(resolve_scenario(&suite.0, "candidate", false), Ok(yaml));
    }
}

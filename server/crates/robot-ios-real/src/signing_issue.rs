use robot_core::UiRunnerStatus;

#[derive(Clone, Debug)]
pub(crate) struct SigningIssue {
    state: &'static str,
    detail: String,
}

impl SigningIssue {
    pub(crate) fn from_xcode(diagnostic: &str, team: Option<&str>) -> Option<Self> {
        let diagnostic = diagnostic.to_ascii_lowercase();
        let (state, action) = if [
            "no account for team",
            "error: no accounts:",
            "unable to log in with account",
            "invalid credentials in keychain",
            "missing xcode-token",
        ]
        .iter()
        .any(|message| diagnostic.contains(message))
        {
            (
                "signing_account_required",
                "Open Xcode → Settings → Apple Accounts and sign in to an Apple account with access to the signing team. Then retry Prepare / Install.",
            )
        } else if diagnostic.contains("no signing certificate") {
            return Some(Self::identity_required(team));
        } else if diagnostic.contains("no profiles for")
            || diagnostic.contains("requires a provisioning profile")
            || diagnostic.contains("doesn't include the currently selected device")
        {
            (
                "signing_profile_required",
                "In Xcode, check Signing & Capabilities and provision this iPhone for the selected team. Then retry Prepare / Install.",
            )
        } else {
            return None;
        };
        Some(Self::new(state, team, action))
    }

    pub(crate) fn identity_required(team: Option<&str>) -> Self {
        Self::new(
            "signing_identity_required",
            team,
            "Install or create an Apple Development certificate for the signing team in Xcode. Then retry Prepare / Install.",
        )
    }

    fn new(state: &'static str, team: Option<&str>, action: &str) -> Self {
        let detail = match team {
            Some(team) => format!("Signing team {team}. {action}"),
            None => action.into(),
        };
        Self { state, detail }
    }

    pub(crate) fn is_signing_state(state: &str) -> bool {
        matches!(
            state,
            "signing_account_required" | "signing_identity_required" | "signing_profile_required"
        )
    }

    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }

    pub(crate) fn apply(self, status: &mut UiRunnerStatus) {
        status.ready = false;
        status.state = self.state.into();
        status.detail = self.detail;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_xcode_account_certificate_and_profile_failures() {
        for (diagnostic, expected) in [
            (
                "error: No Account for Team ABCD123456\nerror: No profiles for app",
                "signing_account_required",
            ),
            (
                "Invalid credentials in keychain, missing Xcode-Token",
                "signing_account_required",
            ),
            (
                "error: No signing certificate was found",
                "signing_identity_required",
            ),
            (
                "error: No profiles for app were found",
                "signing_profile_required",
            ),
        ] {
            let issue = SigningIssue::from_xcode(diagnostic, Some("ABCD123456")).unwrap();
            let mut status = UiRunnerStatus::default();
            issue.apply(&mut status);
            assert_eq!(status.state, expected);
            assert!(status.detail.contains("ABCD123456"));
        }
        assert!(SigningIssue::from_xcode("error: use of unresolved identifier", None).is_none());
    }
}

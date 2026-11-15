//! Stable report values and errors whose classification survives message changes.
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Setup,
    Execution,
    Assertion,
    Unsupported,
    Timeout,
    Interrupted,
    Artifact,
}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Execution => "execution",
            Self::Assertion => "assertion",
            Self::Unsupported => "unsupported",
            Self::Timeout => "timeout",
            Self::Interrupted => "interrupted",
            Self::Artifact => "artifact",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowStatus {
    Pass,
    Fail,
    Partial,
    DryRun,
}

impl FlowStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Partial => "PARTIAL",
            Self::DryRun => "DRY_RUN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Passed,
    Failed,
    Validated,
}

impl StepStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Validated => "validated",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlowError {
    kind: ErrorKind,
    message: String,
    transport: bool,
}

impl FlowError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            transport: false,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub(crate) fn into_transport(mut self) -> Self {
        self.transport = true;
        self
    }

    pub(crate) fn context(mut self, prefix: impl fmt::Display) -> Self {
        self.message = format!("{prefix}: {}", self.message);
        self
    }

    pub(crate) fn during_setup(mut self) -> Self {
        if !matches!(self.kind, ErrorKind::Timeout | ErrorKind::Interrupted) {
            self.kind = ErrorKind::Setup;
        }
        self
    }

    pub(crate) fn for_step(mut self, action: &str) -> Self {
        if self.kind == ErrorKind::Execution && !self.transport && action.starts_with("assert_") {
            self.kind = ErrorKind::Assertion;
        }
        self
    }

    pub(crate) fn append(&mut self, message: impl fmt::Display) {
        self.message.push_str(&format!("; {message}"));
    }
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for FlowError {}
impl From<String> for FlowError {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::Execution, message)
    }
}
impl From<&str> for FlowError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl From<FlowError> for String {
    fn from(error: FlowError) -> Self {
        error.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_and_cleanup_never_change_the_primary_kind() {
        let mut error =
            FlowError::from("expected label contains timeout and MCP").for_step("assert_value");
        error.append("MCP shutdown timeout");
        assert_eq!(error.kind(), ErrorKind::Assertion);
        assert_eq!(
            FlowError::from("timeout is a label").during_setup().kind(),
            ErrorKind::Setup
        );
        assert_eq!(
            FlowError::from("bad response")
                .into_transport()
                .for_step("assert_value")
                .kind(),
            ErrorKind::Execution
        );
        assert_eq!(
            FlowError::new(ErrorKind::Timeout, "deadline exceeded")
                .context("step 1")
                .kind(),
            ErrorKind::Timeout
        );
    }
}

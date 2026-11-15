//! Portable input actions and editor commands.

use crate::RobotError;
use serde::{Deserialize, Serialize};

/// IME action on the focused editor. These are distinct actions; Enter is not
/// a substitute for a named action the field is not configured to perform.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImeAction {
    Next,
    Done,
    Go,
    Search,
    Send,
}

impl ImeAction {
    pub const CONTRACT_NAMES: &'static str = "next, done, go, search, send";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Done => "done",
            Self::Go => "go",
            Self::Search => "search",
            Self::Send => "send",
        }
    }

    /// Keyboard label XCTest should tap. This is an exact key, not a fallback list.
    pub fn keyboard_label(self) -> &'static str {
        match self {
            Self::Next => "Next",
            Self::Done => "Done",
            Self::Go => "Go",
            Self::Search => "Search",
            Self::Send => "Send",
        }
    }

    pub fn parse(value: &str) -> Result<Self, RobotError> {
        match value {
            "next" => Ok(Self::Next),
            "done" => Ok(Self::Done),
            "go" => Ok(Self::Go),
            "search" => Ok(Self::Search),
            "send" => Ok(Self::Send),
            other => Err(RobotError::InvalidInput(format!(
                "unsupported IME action {other:?}; expected one of: {}",
                Self::CONTRACT_NAMES
            ))),
        }
    }

    pub fn from_android_action_id(action_id: u32) -> Option<Self> {
        match action_id {
            2 => Some(Self::Go),
            3 => Some(Self::Search),
            4 => Some(Self::Send),
            5 => Some(Self::Next),
            6 => Some(Self::Done),
            _ => None,
        }
    }
}

/// A UI input operation that can be executed in one driver session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiAction {
    Tap {
        x: f64,
        y: f64,
    },
    TapElement {
        filter: String,
    },
    Type {
        text: String,
    },
    Swipe {
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        duration_ms: Option<u64>,
    },
}

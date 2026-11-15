//! Stable document selectors, validation, and context matching.

use crate::RobotError;
use serde::{Deserialize, Serialize};

/// One hop from a parent document to the iframe or WebView frame it owns.
///
/// Selectors name the owner element in the parent. Runtime frame and session
/// ids are never stored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebFrameSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// One-based position among owners that match the other fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebPageSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Stable document identity for a Chrome frame or an in-app WebView.
///
/// An empty value means the driver's unscoped root document. Old YAML stays
/// unscoped and must not start matching descendant frames.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebContext {
    /// Runtime-only node identity. Never persist in a replay selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<WebPageSelector>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_path: Vec<WebFrameSelector>,
}

impl WebContext {
    pub fn is_empty(&self) -> bool {
        self.app_id.as_deref().unwrap_or("").is_empty()
            && self.page.as_ref().is_none_or(|page| {
                page.url.as_deref().unwrap_or("").is_empty()
                    && page.title.as_deref().unwrap_or("").is_empty()
            })
            && self.frame_path.is_empty()
            && self.element_ref.is_none()
    }

    pub fn validate(&self) -> Result<(), RobotError> {
        for (index, frame) in self.frame_path.iter().enumerate() {
            let css = frame.css.as_deref().unwrap_or("").trim();
            let name = frame.name.as_deref().unwrap_or("").trim();
            let url = frame.url.as_deref().unwrap_or("").trim();
            if css.is_empty() && name.is_empty() && url.is_empty() {
                return Err(RobotError::InvalidInput(format!(
                    "web frame {} needs a css selector, name, or url",
                    index + 1
                )));
            }
            if frame.position.is_some_and(|position| position == 0) {
                return Err(RobotError::InvalidInput(
                    "web frame position is one-based".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Matches stable document constraints without requiring runtime node identity.
pub fn web_context_matches(element: &WebContext, query: &WebContext) -> bool {
    if query.app_id.is_some() && query.app_id != element.app_id {
        return false;
    }
    if let Some(page) = &query.page {
        let Some(actual) = &element.page else {
            return false;
        };
        if page.url.is_some() && page.url != actual.url
            || page.title.is_some() && page.title != actual.title
        {
            return false;
        }
    }
    query.frame_path.len() == element.frame_path.len()
        && query
            .frame_path
            .iter()
            .zip(&element.frame_path)
            .all(|(expected, actual)| {
                (expected.css.is_none() || expected.css == actual.css)
                    && (expected.name.is_none() || expected.name == actual.name)
                    && (expected.url.is_none() || expected.url == actual.url)
                    && (expected.position.is_none() || expected.position == actual.position)
            })
}

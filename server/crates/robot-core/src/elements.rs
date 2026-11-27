//! Accessibility models, classification, and query policy.

use crate::{Bounds, RobotError, WebContext, web_context_matches};
use serde::{Deserialize, Serialize};

/// Normalized control kinds shared by MCP queries and driver adapters.
///
/// `textField` includes password, search, and multiline editors. Drivers that
/// cannot classify a node leave `UiElement::element_type` unset instead of
/// guessing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ElementType {
    TextField,
    Button,
    Link,
    StaticText,
    CheckBox,
    Switch,
    Image,
    Radio,
    Slider,
}

impl ElementType {
    pub const CONTRACT_NAMES: &'static str =
        "textField, button, link, staticText, checkBox, switch, image, radio, slider";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextField => "textField",
            Self::Button => "button",
            Self::Link => "link",
            Self::StaticText => "staticText",
            Self::CheckBox => "checkBox",
            Self::Switch => "switch",
            Self::Image => "image",
            Self::Radio => "radio",
            Self::Slider => "slider",
        }
    }

    pub fn parse(value: &str) -> Result<Self, RobotError> {
        match value {
            "textField" => Ok(Self::TextField),
            "button" => Ok(Self::Button),
            "link" => Ok(Self::Link),
            "staticText" => Ok(Self::StaticText),
            "checkBox" => Ok(Self::CheckBox),
            "switch" => Ok(Self::Switch),
            "image" => Ok(Self::Image),
            "radio" => Ok(Self::Radio),
            "slider" => Ok(Self::Slider),
            other => Err(RobotError::InvalidInput(format!(
                "unsupported elementType {other:?}; expected one of: {}",
                Self::CONTRACT_NAMES
            ))),
        }
    }
}

/// Owned query so drivers can apply type and text before `max_results`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementQuery {
    pub filter: Option<String>,
    pub element_type: Option<ElementType>,
    pub max_results: Option<usize>,
    /// When set, only elements in this document are eligible.
    pub web_context: Option<WebContext>,
    /// Live inspection opts into descendant frames. Unscoped queries stay on the root document.
    pub include_frames: bool,
    /// Omit system keyboard nodes. Sources without this capability return
    /// `RobotError::Unsupported` for an explicit request.
    pub exclude_system_keyboard: bool,
}

impl ElementQuery {
    /// Reject a document selector when the element source only exposes native nodes.
    pub fn reject_web_context(&self, device_id: &str) -> Result<(), RobotError> {
        if self
            .web_context
            .as_ref()
            .is_some_and(|context| !context.is_empty())
        {
            return Err(RobotError::Unsupported(format!(
                "web context queries are not supported for {device_id}"
            )));
        }
        Ok(())
    }

    /// Call before reading from a source that cannot exclude keyboard nodes.
    pub fn reject_keyboard_exclusion(&self) -> Result<(), RobotError> {
        if self.exclude_system_keyboard {
            return Err(RobotError::Unsupported(
                "system keyboard exclusion is not supported by this element source".into(),
            ));
        }
        Ok(())
    }
}

/// A normalized accessibility node from the platform UI tree.
///
/// `focused`, `checked`, and `selected` are omitted from JSON when the driver
/// could not observe them. Absence means unknown, not false.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiElement {
    pub class_name: String,
    pub resource_id: Option<String>,
    pub text: Option<String>,
    pub content_description: Option<String>,
    pub bounds: Bounds,
    pub clickable: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    /// Native editor value when the platform exposes one separately from the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_type: Option<ElementType>,
    /// Present when the node was read from a descendant frame or in-app WebView.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_context: Option<WebContext>,
}

/// Fills `element_type` from the native class or role when the adapter did not.
pub fn prepare_elements(elements: &mut [UiElement]) {
    for element in elements {
        if element.element_type.is_none() {
            element.element_type = classify_class_name(&element.class_name);
        }
    }
}

pub fn classify_class_name(class_name: &str) -> Option<ElementType> {
    let simple = class_name.rsplit(['.', '/']).next().unwrap_or(class_name);
    let normalized = simple.to_ascii_lowercase().replace('_', "");
    if normalized.contains("edittext")
        || normalized.contains("autocomplete")
        || matches!(
            normalized.as_str(),
            "textfield"
                | "securetextfield"
                | "textarea"
                | "searchfield"
                | "axtextfield"
                | "axtextarea"
                | "axsearchfield"
        )
    {
        return Some(ElementType::TextField);
    }
    if normalized.contains("checkbox") || normalized.contains("checkedtext") {
        return Some(ElementType::CheckBox);
    }
    if normalized.contains("radiobutton")
        || matches!(normalized.as_str(), "radio" | "axradiobutton")
    {
        return Some(ElementType::Radio);
    }
    if normalized.contains("switch") || normalized.contains("toggle") {
        return Some(ElementType::Switch);
    }
    if normalized.contains("seekbar") || matches!(normalized.as_str(), "slider" | "axslider") {
        return Some(ElementType::Slider);
    }
    if normalized.contains("button") {
        return Some(ElementType::Button);
    }
    if normalized.contains("image") || normalized == "img" {
        return Some(ElementType::Image);
    }
    if matches!(normalized.as_str(), "link" | "a" | "axlink") {
        return Some(ElementType::Link);
    }
    if matches!(
        normalized.as_str(),
        "text" | "statictext" | "textview" | "axstatictext"
    ) {
        return Some(ElementType::StaticText);
    }
    None
}

pub fn element_matches_filter(element: &UiElement, filter: &str) -> bool {
    element_fields_match_filter(
        [
            Some(element.class_name.as_str()),
            element.resource_id.as_deref(),
            element.text.as_deref(),
            element.content_description.as_deref(),
            element.value.as_deref(),
        ],
        filter,
    )
}

/// Matches projected accessibility fields before a driver reads the rest of a node.
pub fn element_fields_match_filter<const N: usize>(
    fields: [Option<&str>; N],
    filter: &str,
) -> bool {
    let filter = filter.to_lowercase();
    fields
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&filter))
}

/// Filters document scope, text, and type before applying `max_results`.
pub fn apply_element_query(mut elements: Vec<UiElement>, query: &ElementQuery) -> Vec<UiElement> {
    prepare_elements(&mut elements);
    if let Some(context) = query
        .web_context
        .as_ref()
        .filter(|context| !context.is_empty())
    {
        elements.retain(|element| {
            element
                .web_context
                .as_ref()
                .is_some_and(|candidate| web_context_matches(candidate, context))
        });
    } else if !query.include_frames {
        elements.retain(|element| {
            element
                .web_context
                .as_ref()
                .is_none_or(|context| context.frame_path.is_empty() && context.app_id.is_none())
        });
    }
    if let Some(filter) = query.filter.as_deref() {
        elements.retain(|element| element_matches_filter(element, filter));
    }
    if let Some(element_type) = query.element_type {
        elements.retain(|element| element.element_type == Some(element_type));
    }
    if let Some(max_results) = query.max_results {
        elements.truncate(max_results);
    }
    elements
}

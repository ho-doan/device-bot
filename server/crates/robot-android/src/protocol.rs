//! Decoders for ADB and UI Automator output.

use std::collections::HashMap;

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use robot_core::{
    Bounds, DeviceState, ElementType, ImeAction, Platform, RobotDevice, RobotError, UiElement,
    prepare_elements,
};
use serde_json::{Value, json};

pub fn parse_uiautomator_xml(xml: &str) -> Result<Vec<UiElement>, RobotError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut elements = vec![];

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(node)) | Ok(Event::Start(node)) if node.name().as_ref() == b"node" => {
                elements.push(parse_node(&node, reader.decoder())?);
            }
            Ok(_) => {}
            Err(error) => {
                return Err(RobotError::CommandFailed(format!(
                    "could not parse uiautomator XML: {error}"
                )));
            }
        }
    }
    Ok(elements)
}

fn parse_node(
    node: &BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<UiElement, RobotError> {
    let mut attributes = HashMap::new();
    for attribute in node.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            RobotError::CommandFailed(format!("could not parse uiautomator attribute: {error}"))
        })?;
        let key = std::str::from_utf8(attribute.key.as_ref()).map_err(|error| {
            RobotError::CommandFailed(format!("invalid attribute name: {error}"))
        })?;
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|error| {
                RobotError::CommandFailed(format!("invalid attribute value: {error}"))
            })?;
        attributes.insert(key.to_owned(), value.into_owned());
    }

    let bounds = attributes
        .get("bounds")
        .ok_or_else(|| RobotError::CommandFailed("uiautomator node has no bounds".into()))
        .and_then(|value| parse_bounds(value))?;
    let text = optional_attribute(&mut attributes, "text");
    let mut element = UiElement {
        class_name: attributes.remove("class").unwrap_or_default(),
        resource_id: optional_attribute(&mut attributes, "resource-id"),
        text,
        content_description: optional_attribute(&mut attributes, "content-desc"),
        bounds,
        clickable: attributes
            .get("clickable")
            .is_some_and(|value| value == "true"),
        enabled: attributes
            .get("enabled")
            .is_none_or(|value| value == "true"),
        focused: optional_bool(&attributes, "focused"),
        checked: optional_bool(&attributes, "checked"),
        selected: optional_bool(&attributes, "selected"),
        value: None,
        element_type: None,
        web_context: None,
    };
    prepare_elements(std::slice::from_mut(&mut element));
    if element.element_type == Some(ElementType::TextField) {
        element.value = element.text.clone();
    }
    Ok(element)
}

fn optional_bool(attributes: &HashMap<String, String>, key: &str) -> Option<bool> {
    match attributes.get(key).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// Reads the focused editor's `imeOptions` action. A null served view or an
/// action other than next/done/go/search/send is an error so callers do not
/// send KEYCODE_ENTER for a different IME action.
pub fn parse_focused_ime_action(dumpsys: &str) -> Result<ImeAction, RobotError> {
    if last_assignment(dumpsys, "mServedView")
        .is_some_and(|value| value == "null" || value.is_empty())
    {
        return Err(RobotError::CommandFailed(
            "no focused editor is available for an IME action".into(),
        ));
    }
    let raw = last_ime_options(dumpsys).ok_or_else(|| {
        RobotError::CommandFailed(
            "focused editor did not report imeOptions; KEYCODE_ENTER was not sent".into(),
        )
    })?;
    let action_id = raw & 0xff;
    ImeAction::from_android_action_id(action_id).ok_or_else(|| {
        RobotError::CommandFailed(format!(
            "focused editor imeOptions action id {action_id} is not next, done, go, search, or send; KEYCODE_ENTER was not sent"
        ))
    })
}

fn last_assignment<'a>(dumpsys: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=");
    let mut found = None;
    let mut rest = dumpsys;
    while let Some(index) = rest.find(&pattern) {
        let after = &rest[index + pattern.len()..];
        let token = after.split_whitespace().next().unwrap_or("");
        found = Some(token.trim_matches(|character| character == ',' || character == '}'));
        rest = after.get(token.len()..).unwrap_or("");
    }
    found
}

fn last_ime_options(dumpsys: &str) -> Option<u32> {
    let mut found = None;
    let mut rest = dumpsys;
    while let Some(index) = rest.find("imeOptions=") {
        let after = &rest[index + "imeOptions=".len()..];
        let token = after
            .split(|character: char| {
                character.is_whitespace() || matches!(character, ',' | '}' | '"' | ';')
            })
            .next()
            .unwrap_or("");
        if let Some(value) = parse_ime_token(token) {
            found = Some(value);
        }
        rest = after.get(token.len()..).unwrap_or("");
    }
    found
}

fn parse_ime_token(token: &str) -> Option<u32> {
    let token = token.trim();
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        token.parse().ok()
    }
}

fn optional_attribute(attributes: &mut HashMap<String, String>, key: &str) -> Option<String> {
    attributes.remove(key).filter(|value| !value.is_empty())
}

fn parse_bounds(value: &str) -> Result<Bounds, RobotError> {
    let values: Result<Vec<i32>, _> = value
        .split(['[', ']', ','])
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect();
    let values = values.map_err(|error| {
        RobotError::CommandFailed(format!("invalid uiautomator bounds {value:?}: {error}"))
    })?;
    match values.as_slice() {
        [left, top, right, bottom] => Ok(Bounds {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
        }),
        _ => Err(RobotError::CommandFailed(format!(
            "invalid uiautomator bounds: {value:?}"
        ))),
    }
}

pub fn parse_adb_devices(output: &str) -> Vec<RobotDevice> {
    output.lines().filter_map(parse_device_line).collect()
}

/// Parses `adb shell pm list packages -f` output into portable app records.
pub fn parse_adb_installed_apps(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| {
            let package = line.trim().strip_prefix("package:")?;
            let (source_path, package_name) = package.rsplit_once('=')?;
            Some(json!({
                "packageName": package_name,
                "sourcePath": source_path,
            }))
        })
        .collect()
}

fn parse_device_line(line: &str) -> Option<RobotDevice> {
    let mut fields = line.split_whitespace();
    let id = fields.next()?;
    let state = fields.next()?;
    if id == "List" || id.starts_with('*') {
        return None;
    }

    let attributes: Vec<_> = fields.collect();
    let name = attributes
        .iter()
        .find_map(|field| field.strip_prefix("model:"))
        .map(|model| model.replace('_', " "))
        .unwrap_or_else(|| id.to_owned());

    Some(RobotDevice {
        id: id.to_owned(),
        name,
        platform: Platform::Android,
        state: match state {
            "device" => DeviceState::Available,
            "offline" => DeviceState::Offline,
            "unauthorized" => DeviceState::Unauthorized,
            _ => DeviceState::Unknown,
        },
    })
}

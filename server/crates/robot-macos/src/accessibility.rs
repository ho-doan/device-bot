//! Native accessibility traversal, focus, hit testing, and target window selection.

use super::ffi::{
    AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementCreateSystemWide,
    AXUIElementGetPid, AXUIElementPerformAction, AXUIElementRef, AXUIElementSetAttributeValue,
    AXValueGetValue, CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFBooleanGetTypeID,
    CFBooleanGetValue, CFDictionaryGetValue, CFGetTypeID, CFNumberGetValue, CFRelease, CFRetain,
    CFStringCreateWithBytes, CFStringGetCString, CFStringGetLength,
    CFStringGetMaximumSizeForEncoding, CFStringGetTypeID, CFStringRef, CFTypeRef, CGPoint, CGSize,
    CGWindowListCopyWindowInfo, CfOwned, K_AX_VALUE_CG_POINT_TYPE, K_AX_VALUE_CG_SIZE_TYPE,
    K_CF_NUMBER_SINT32_TYPE, K_CF_STRING_ENCODING_UTF8, K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
    K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, kCFBooleanTrue,
};
use super::windows::native_listed_windows;
use super::{NATIVE_FILTER_BUDGET, non_empty, request_accessibility_permission};
use robot_core::{Point, RobotError, UiElement};
use std::{ffi::CStr, os::raw::c_long, ptr, time::Instant};

pub(super) struct NativeAxAttributes {
    pub(super) manual_accessibility: CFStringRef,
    pub(super) focused_window: CFStringRef,
    pub(super) focused: CFStringRef,
    pub(super) enabled: CFStringRef,
    pub(super) windows: CFStringRef,
    pub(super) children: CFStringRef,
    pub(super) role: CFStringRef,
    pub(super) title: CFStringRef,
    pub(super) description: CFStringRef,
    pub(super) value: CFStringRef,
    pub(super) position: CFStringRef,
    pub(super) size: CFStringRef,
}

impl NativeAxAttributes {
    pub(super) fn new() -> Result<Self, RobotError> {
        Ok(Self {
            manual_accessibility: native_cf_string("AXManualAccessibility")?,
            focused_window: native_cf_string("AXFocusedWindow")?,
            focused: native_cf_string("AXFocused")?,
            enabled: native_cf_string("AXEnabled")?,
            windows: native_cf_string("AXWindows")?,
            children: native_cf_string("AXChildren")?,
            role: native_cf_string("AXRole")?,
            title: native_cf_string("AXTitle")?,
            description: native_cf_string("AXDescription")?,
            value: native_cf_string("AXValue")?,
            position: native_cf_string("AXPosition")?,
            size: native_cf_string("AXSize")?,
        })
    }
}

impl Drop for NativeAxAttributes {
    fn drop(&mut self) {
        for value in [
            self.manual_accessibility,
            self.focused_window,
            self.focused,
            self.enabled,
            self.windows,
            self.children,
            self.role,
            self.title,
            self.description,
            self.value,
            self.position,
            self.size,
        ] {
            unsafe { CFRelease(value) };
        }
    }
}

pub(super) fn native_cf_string(value: &str) -> Result<CFStringRef, RobotError> {
    let string = unsafe {
        CFStringCreateWithBytes(
            ptr::null(),
            value.as_ptr(),
            value.len() as c_long,
            K_CF_STRING_ENCODING_UTF8,
            false,
        )
    };
    if string.is_null() {
        Err(RobotError::CommandFailed(
            "could not allocate a macOS accessibility attribute name".into(),
        ))
    } else {
        Ok(string)
    }
}

pub(super) fn native_copy_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
) -> Option<CfOwned> {
    let mut value = ptr::null();
    (unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) } == 0
        && !value.is_null())
    .then_some(CfOwned(value))
}

pub(super) fn native_required_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
    name: &str,
) -> Result<CfOwned, RobotError> {
    let mut error = 0;
    for attempt in 0..20 {
        let mut value = ptr::null();
        error = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        if error == 0 && !value.is_null() {
            return Ok(CfOwned(value));
        }
        // kAXErrorCannotComplete is transient while a newly launched app is
        // publishing its focused window to the Accessibility server.
        if error != -25204 || attempt == 19 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(RobotError::CommandFailed(format!(
        "could not read macOS accessibility attribute {name} (AXError {error})"
    )))
}

pub(super) fn native_string(value: CFTypeRef) -> Option<String> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let length = unsafe { CFStringGetLength(value) };
    let capacity = unsafe {
        CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8).saturating_add(1)
    };
    let mut bytes = vec![0_i8; capacity.max(1) as usize];
    if unsafe {
        CFStringGetCString(
            value,
            bytes.as_mut_ptr(),
            capacity,
            K_CF_STRING_ENCODING_UTF8,
        )
    } {
        unsafe { CStr::from_ptr(bytes.as_ptr()) }
            .to_str()
            .ok()
            .map(str::to_owned)
    } else {
        None
    }
}

pub(super) fn native_i32(value: CFTypeRef) -> Option<i32> {
    if value.is_null() {
        return None;
    }
    let mut result = 0_i32;
    unsafe {
        CFNumberGetValue(
            value,
            K_CF_NUMBER_SINT32_TYPE,
            (&mut result as *mut i32).cast(),
        )
    }
    .then_some(result)
}

pub(super) fn native_frontmost_application(
    preferred_pid: Option<i32>,
) -> Result<CfOwned, RobotError> {
    if let Some(pid) = preferred_pid {
        let application = CfOwned(unsafe { AXUIElementCreateApplication(pid) });
        if pid > 0 && !application.0.is_null() {
            return Ok(application);
        }
        return Err(RobotError::CommandFailed(
            "the selected macOS target process is unavailable; select the app again".into(),
        ));
    }
    // Query the Accessibility server each time. NSWorkspace notifications can
    // remain stale on a headless Tokio worker without a Cocoa run loop.
    let system = CfOwned(unsafe { AXUIElementCreateSystemWide() });
    let focused_app = CfOwned(native_cf_string("AXFocusedApplication")?);
    if !system.0.is_null() {
        if let Some(application) = native_copy_attribute(system.0, focused_app.0) {
            let mut pid = 0;
            if unsafe { AXUIElementGetPid(application.0, &mut pid) } == 0 && pid > 0 {
                return Ok(application);
            }
        }
    }

    let windows = CfOwned(unsafe {
        CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        )
    });
    if windows.0.is_null() || unsafe { CFGetTypeID(windows.0) } != unsafe { CFArrayGetTypeID() } {
        return Err(RobotError::CommandFailed(
            "could not enumerate macOS windows to locate the foreground application".into(),
        ));
    }
    let layer_key = native_cf_string("kCGWindowLayer")?;
    let owner_pid_key = native_cf_string("kCGWindowOwnerPID")?;
    let count = unsafe { CFArrayGetCount(windows.0) };
    for index in 0..count {
        let window = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
        if window.is_null() {
            continue;
        }
        let layer = native_i32(unsafe { CFDictionaryGetValue(window, layer_key) });
        if layer != Some(0) {
            continue;
        }
        let Some(pid) = native_i32(unsafe { CFDictionaryGetValue(window, owner_pid_key) }) else {
            continue;
        };
        let application = CfOwned(unsafe { AXUIElementCreateApplication(pid) });
        if !application.0.is_null() {
            return Ok(application);
        }
    }
    Err(RobotError::CommandFailed(
        "could not locate a foreground application window".into(),
    ))
}

fn native_string_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<String> {
    native_copy_attribute(element, attribute).and_then(|value| native_string(value.0))
}

pub(super) fn native_point_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
) -> Option<CGPoint> {
    let value = native_copy_attribute(element, attribute)?;
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    unsafe {
        AXValueGetValue(
            value.0,
            K_AX_VALUE_CG_POINT_TYPE,
            (&mut point as *mut CGPoint).cast(),
        )
    }
    .then_some(point)
}

pub(super) fn native_size_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
) -> Option<CGSize> {
    let value = native_copy_attribute(element, attribute)?;
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    unsafe {
        AXValueGetValue(
            value.0,
            K_AX_VALUE_CG_SIZE_TYPE,
            (&mut size as *mut CGSize).cast(),
        )
    }
    .then_some(size)
}

fn native_matches_filter(
    filter: Option<&str>,
    role: &str,
    text: Option<&str>,
    description: Option<&str>,
) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    [Some(role), text, description]
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(filter))
}

fn native_bool_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<bool> {
    let value = native_copy_attribute(element, attribute)?;
    (unsafe { CFGetTypeID(value.0) } == unsafe { CFBooleanGetTypeID() })
        .then(|| unsafe { CFBooleanGetValue(value.0) })
}

fn native_is_editor(role: &str) -> bool {
    matches!(role, "AXTextField" | "AXTextArea" | "AXComboBox")
}

pub(super) fn native_collect_elements(
    element: AXUIElementRef,
    attributes: &NativeAxAttributes,
    filter: Option<&str>,
    max_results: Option<usize>,
    deadline: Instant,
    depth: usize,
    elements: &mut Vec<UiElement>,
) {
    if Instant::now() >= deadline
        || max_results.is_some_and(|max| elements.len() >= max)
        || depth > 128
    {
        return;
    }

    let role =
        native_string_attribute(element, attributes.role).unwrap_or_else(|| "AXUnknown".into());
    let title =
        native_string_attribute(element, attributes.title).and_then(|value| non_empty(&value));
    let description = native_string_attribute(element, attributes.description)
        .and_then(|value| non_empty(&value));
    let value = native_string_attribute(element, attributes.value);
    let enabled = native_bool_attribute(element, attributes.enabled).unwrap_or(true);
    let text = title.clone().or_else(|| value.clone());

    if native_matches_filter(filter, &role, text.as_deref(), description.as_deref()) {
        if let (Some(position), Some(size)) = (
            native_point_attribute(element, attributes.position),
            native_size_attribute(element, attributes.size),
        ) {
            if size.width > 0.0 && size.height > 0.0 {
                elements.push(UiElement {
                    class_name: role.clone(),
                    resource_id: None,
                    text,
                    content_description: description,
                    bounds: robot_core::Bounds {
                        left: position.x.round() as i32,
                        top: position.y.round() as i32,
                        right: (position.x + size.width).round() as i32,
                        bottom: (position.y + size.height).round() as i32,
                    },
                    clickable: enabled
                        && (native_is_editor(&role)
                            || matches!(
                                role.as_str(),
                                "AXButton"
                                    | "AXLink"
                                    | "AXMenuItem"
                                    | "AXPopUpButton"
                                    | "AXRadioButton"
                            )),
                    enabled,
                    focused: native_bool_attribute(element, attributes.focused),
                    checked: None,
                    selected: None,
                    value,
                    element_type: None,
                    web_context: None,
                });
            }
        }
    }

    if max_results.is_some_and(|max| elements.len() >= max) {
        return;
    }
    let Some(children) = native_copy_attribute(element, attributes.children) else {
        return;
    };
    if unsafe { CFGetTypeID(children.0) } != unsafe { CFArrayGetTypeID() } {
        return;
    }
    let count = unsafe { CFArrayGetCount(children.0) };
    for index in 0..count {
        let child = unsafe { CFArrayGetValueAtIndex(children.0, index) };
        if !child.is_null() {
            native_collect_elements(
                child,
                attributes,
                filter,
                max_results,
                deadline,
                depth + 1,
                elements,
            );
        }
        if Instant::now() >= deadline || max_results.is_some_and(|max| elements.len() >= max) {
            break;
        }
    }
}

pub(super) fn native_bounds_contains(
    element: AXUIElementRef,
    attributes: &NativeAxAttributes,
    point: Point,
) -> Option<bool> {
    let position = native_point_attribute(element, attributes.position)?;
    let size = native_size_attribute(element, attributes.size)?;
    Some(
        point.x >= position.x
            && point.x <= position.x + size.width
            && point.y >= position.y
            && point.y <= position.y + size.height,
    )
}

fn native_bounds_center_matches(
    element: AXUIElementRef,
    attributes: &NativeAxAttributes,
    point: Point,
) -> bool {
    let Some(position) = native_point_attribute(element, attributes.position) else {
        return false;
    };
    let Some(size) = native_size_attribute(element, attributes.size) else {
        return false;
    };
    ((position.x + size.width / 2.0) - point.x).abs() <= 1.0
        && ((position.y + size.height / 2.0) - point.y).abs() <= 1.0
}

fn native_press_at(
    element: AXUIElementRef,
    attributes: &NativeAxAttributes,
    press: CFStringRef,
    point: Point,
    depth: usize,
) -> bool {
    let contains = native_bounds_contains(element, attributes, point);
    if depth > 128 || contains == Some(false) {
        return false;
    }
    if let Some(children) = native_copy_attribute(element, attributes.children) {
        if unsafe { CFGetTypeID(children.0) } == unsafe { CFArrayGetTypeID() } {
            for index in 0..unsafe { CFArrayGetCount(children.0) } {
                let child = unsafe { CFArrayGetValueAtIndex(children.0, index) };
                if !child.is_null() && native_press_at(child, attributes, press, point, depth + 1) {
                    return true;
                }
            }
        }
    }
    let role = native_string_attribute(element, attributes.role).unwrap_or_default();
    if contains != Some(true)
        || !native_bounds_center_matches(element, attributes, point)
        || native_bool_attribute(element, attributes.enabled) == Some(false)
    {
        return false;
    }
    if native_is_editor(&role) {
        // Focus the native editor; AXValue writes can bypass application delegates.
        // Text is sent separately through real keyboard events.
        return unsafe {
            AXUIElementSetAttributeValue(element, attributes.focused, kCFBooleanTrue)
        } == 0;
    }
    matches!(
        role.as_str(),
        "AXButton" | "AXLink" | "AXMenuItem" | "AXPopUpButton" | "AXRadioButton"
    ) && unsafe { AXUIElementPerformAction(element, press) } == 0
}

pub(super) fn native_accessibility_tap(
    point: Point,
    preferred_pid: Option<i32>,
) -> Result<(), RobotError> {
    let attributes = NativeAxAttributes::new()?;
    let application = native_frontmost_application(preferred_pid)?;
    let _ = unsafe {
        AXUIElementSetAttributeValue(
            application.0,
            attributes.manual_accessibility,
            kCFBooleanTrue,
        )
    };
    let press = native_cf_string("AXPress")?;
    let window =
        native_required_attribute(application.0, attributes.focused_window, "AXFocusedWindow")?;
    native_press_at(window.0, &attributes, press, point, 0)
        .then_some(())
        .ok_or_else(|| {
            RobotError::CommandFailed(format!(
                "could not find an actionable macOS accessibility element at {}, {}",
                point.x, point.y
            ))
        })
}

pub(super) fn native_accessibility_elements(
    filter: Option<&str>,
    max_results: Option<usize>,
    preferred_pid: Option<i32>,
    preferred_window: Option<String>,
) -> Result<Vec<UiElement>, RobotError> {
    if max_results == Some(0) {
        return Ok(vec![]);
    }
    if !request_accessibility_permission() {
        return Err(RobotError::CommandFailed(
            "Accessibility permission is required for the MCP server process. macOS has been asked to grant it; approve the prompt, then retry."
                .into(),
        ));
    }
    let attributes = NativeAxAttributes::new()?;
    let application = native_frontmost_application(preferred_pid)?;
    // Chrome does not expose renderer DOM nodes through AX until this is set.
    let _ = unsafe {
        AXUIElementSetAttributeValue(
            application.0,
            attributes.manual_accessibility,
            kCFBooleanTrue,
        )
    };
    let window = native_target_window(application.0, &attributes, preferred_window.as_deref())?;
    let mut elements = Vec::new();
    native_collect_elements(
        window.0,
        &attributes,
        filter,
        max_results,
        Instant::now() + NATIVE_FILTER_BUDGET,
        0,
        &mut elements,
    );
    Ok(elements)
}

fn native_target_window(
    application: AXUIElementRef,
    attributes: &NativeAxAttributes,
    preferred_window: Option<&str>,
) -> Result<CfOwned, RobotError> {
    let windows = native_copy_attribute(application, attributes.windows);
    if let Some(preferred) = preferred_window.filter(|value| !value.is_empty()) {
        if let Some(windows) = windows.as_ref() {
            if unsafe { CFGetTypeID(windows.0) } == unsafe { CFArrayGetTypeID() } {
                let count = unsafe { CFArrayGetCount(windows.0) };
                for index in 0..count {
                    let child = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
                    if child.is_null() {
                        continue;
                    }
                    let title =
                        native_string_attribute(child, attributes.title).unwrap_or_default();
                    let listed_title = native_listed_windows().ok().and_then(|windows| {
                        windows
                            .into_iter()
                            .find(|window| window.window_id == preferred)
                            .map(|window| window.title)
                    });
                    if title == preferred
                        || listed_title
                            .as_ref()
                            .is_some_and(|value| !value.is_empty() && value == &title)
                        || preferred == index.to_string()
                    {
                        return Ok(CfOwned(unsafe { CFRetain(child) }));
                    }
                }
            }
        }
        return Err(RobotError::CommandFailed(format!(
            "could not find macOS window {preferred}; list windows again and select a visible one"
        )));
    }
    match native_required_attribute(application, attributes.focused_window, "AXFocusedWindow") {
        Ok(window) => Ok(window),
        Err(_) => {
            let Some(windows) = windows else {
                return Err(RobotError::CommandFailed(
                    "the selected macOS app has no focused window and no AXWindows; show a window, then retry".into(),
                ));
            };
            if unsafe { CFGetTypeID(windows.0) } != unsafe { CFArrayGetTypeID() }
                || unsafe { CFArrayGetCount(windows.0) } == 0
            {
                return Err(RobotError::CommandFailed(
                    "the selected macOS app has no focused window; show a window, then retry"
                        .into(),
                ));
            }
            let child = unsafe { CFArrayGetValueAtIndex(windows.0, 0) };
            if child.is_null() {
                return Err(RobotError::CommandFailed(
                    "the selected macOS app has no focused window; show a window, then retry"
                        .into(),
                ));
            }
            Ok(CfOwned(unsafe { CFRetain(child) }))
        }
    }
}

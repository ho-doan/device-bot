//! Native macOS desktop driver.
//!
//! Input is performed through `System Events`, so macOS will require the MCP
//! server process to have Accessibility permission before pointer or keyboard
//! commands can succeed.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "macos")]
use std::{
    ffi::{CStr, c_char, c_void},
    os::raw::c_long,
    ptr,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use robot_core::{DeviceState, Platform, Point, Robot, RobotDevice, RobotError, UiElement};
use serde_json::{Value, json};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

const DEVICE_ID: &str = "macos-host";
#[cfg(target_os = "macos")]
const NATIVE_FILTER_BUDGET: Duration = Duration::from_secs(2);
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

#[cfg(target_os = "macos")]
type CFTypeRef = *const c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;
#[cfg(target_os = "macos")]
type CFArrayRef = *const c_void;
#[cfg(target_os = "macos")]
type CFDictionaryRef = *const c_void;
#[cfg(target_os = "macos")]
type AXUIElementRef = *const c_void;

#[cfg(target_os = "macos")]
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
#[cfg(target_os = "macos")]
const K_AX_VALUE_CG_POINT_TYPE: u32 = 1;
#[cfg(target_os = "macos")]
const K_AX_VALUE_CG_SIZE_TYPE: u32 = 2;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
#[cfg(target_os = "macos")]
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 16;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> i32;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, result: *mut c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    fn CFRelease(value: CFTypeRef);
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: c_long,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFDictionaryRef;
    fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: CFArrayRef) -> c_long;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: c_long) -> CFTypeRef;
    fn CFGetTypeID(value: CFTypeRef) -> usize;
    fn CFNumberGetValue(number: CFTypeRef, number_type: i32, value: *mut c_void) -> bool;
    fn CFStringGetTypeID() -> usize;
    fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: c_long,
        encoding: u32,
        is_external_representation: bool,
    ) -> CFStringRef;
    fn CFStringGetLength(string: CFStringRef) -> c_long;
    fn CFStringGetMaximumSizeForEncoding(length: c_long, encoding: u32) -> c_long;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: c_long,
        encoding: u32,
    ) -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
    #[link_name = "objc_msgSend"]
    fn objc_msg_send(receiver: *mut c_void, selector: *const c_void) -> usize;
}

#[cfg(target_os = "macos")]
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSApplicationLoad() -> bool;
}

/// Requests the macOS Screen Recording permission when it has not yet been
/// granted. macOS owns the prompt and can require a retry after approval.
pub fn request_screen_recording_permission() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Prompts for Accessibility permission when the MCP process is not trusted.
/// macOS owns the consent flow, so callers must retry after the user approves.
pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            ptr::null(),
            ptr::null(),
        );
        if options.is_null() {
            return false;
        }
        let trusted = AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
        trusted
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
struct CfOwned(CFTypeRef);

#[cfg(target_os = "macos")]
impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeAxAttributes {
    manual_accessibility: CFStringRef,
    focused_window: CFStringRef,
    children: CFStringRef,
    role: CFStringRef,
    title: CFStringRef,
    description: CFStringRef,
    value: CFStringRef,
    position: CFStringRef,
    size: CFStringRef,
}

#[cfg(target_os = "macos")]
impl NativeAxAttributes {
    fn new() -> Result<Self, RobotError> {
        Ok(Self {
            manual_accessibility: native_cf_string("AXManualAccessibility")?,
            focused_window: native_cf_string("AXFocusedWindow")?,
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

#[cfg(target_os = "macos")]
impl Drop for NativeAxAttributes {
    fn drop(&mut self) {
        for value in [
            self.manual_accessibility,
            self.focused_window,
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

#[cfg(target_os = "macos")]
fn native_cf_string(value: &str) -> Result<CFStringRef, RobotError> {
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

#[cfg(target_os = "macos")]
fn native_copy_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<CfOwned> {
    let mut value = ptr::null();
    (unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) } == 0
        && !value.is_null())
    .then_some(CfOwned(value))
}

#[cfg(target_os = "macos")]
fn native_required_attribute(
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

#[cfg(target_os = "macos")]
fn native_string(value: CFTypeRef) -> Option<String> {
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

#[cfg(target_os = "macos")]
fn native_i32(value: CFTypeRef) -> Option<i32> {
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

#[cfg(target_os = "macos")]
fn native_frontmost_application(preferred_pid: Option<i32>) -> Result<CfOwned, RobotError> {
    if let Some(pid) = preferred_pid {
        let application = CfOwned(unsafe { AXUIElementCreateApplication(pid) });
        if pid > 0 && !application.0.is_null() {
            return Ok(application);
        }
        return Err(RobotError::CommandFailed(
            "the selected macOS target process is unavailable; select the app again".into(),
        ));
    }
    // NSWorkspace identifies the actual foreground application PID even when
    // several Chrome processes have the same owner name.
    unsafe { NSApplicationLoad() };
    let workspace_class = unsafe { objc_getClass(c"NSWorkspace".as_ptr()) };
    let shared_workspace = unsafe {
        objc_msg_send(
            workspace_class,
            sel_registerName(c"sharedWorkspace".as_ptr()),
        ) as *mut c_void
    };
    let frontmost_application = unsafe {
        objc_msg_send(
            shared_workspace,
            sel_registerName(c"frontmostApplication".as_ptr()),
        ) as *mut c_void
    };
    let pid = unsafe {
        objc_msg_send(
            frontmost_application,
            sel_registerName(c"processIdentifier".as_ptr()),
        ) as i32
    };
    if pid > 0 {
        let application = CfOwned(unsafe { AXUIElementCreateApplication(pid) });
        if !application.0.is_null() {
            return Ok(application);
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

#[cfg(target_os = "macos")]
fn native_string_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<String> {
    native_copy_attribute(element, attribute).and_then(|value| native_string(value.0))
}

#[cfg(target_os = "macos")]
fn native_point_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<CGPoint> {
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

#[cfg(target_os = "macos")]
fn native_size_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<CGSize> {
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn native_collect_elements(
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
    let value =
        native_string_attribute(element, attributes.value).and_then(|value| non_empty(&value));
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
                    clickable: matches!(
                        role.as_str(),
                        "AXButton" | "AXLink" | "AXMenuItem" | "AXPopUpButton"
                    ),
                    enabled: true,
                    focused: None,
                    checked: None,
                    selected: None,
                    value,
                    element_type: None,
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

#[cfg(target_os = "macos")]
fn native_bounds_contains(
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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
    contains == Some(true)
        && native_bounds_center_matches(element, attributes, point)
        && matches!(
            role.as_str(),
            "AXButton"
                | "AXLink"
                | "AXMenuItem"
                | "AXPopUpButton"
                | "AXRadioButton"
                | "AXTextField"
        )
        && unsafe { AXUIElementPerformAction(element, press) } == 0
}

#[cfg(target_os = "macos")]
fn native_accessibility_tap(point: Point, preferred_pid: Option<i32>) -> Result<(), RobotError> {
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

#[cfg(target_os = "macos")]
fn native_accessibility_elements(
    filter: Option<&str>,
    max_results: Option<usize>,
    preferred_pid: Option<i32>,
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
    let window =
        native_required_attribute(application.0, attributes.focused_window, "AXFocusedWindow")?;
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

#[derive(Debug)]
struct ActiveRecording {
    path: PathBuf,
    child: Child,
}

#[derive(Debug, Clone)]
pub struct MacosRobot {
    artifact_dir: PathBuf,
    ffmpeg_path: String,
    osascript_path: String,
    open_path: String,
    screencapture_path: String,
    recording: Arc<Mutex<Option<ActiveRecording>>>,
    active_application: Arc<Mutex<Option<String>>>,
    active_process_id: Arc<Mutex<Option<i32>>>,
    capture_lock: Arc<Mutex<()>>,
}

impl MacosRobot {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifact_dir: artifact_dir.into(),
            ffmpeg_path: Self::ffmpeg_path(),
            osascript_path: "osascript".into(),
            open_path: "open".into(),
            screencapture_path: "screencapture".into(),
            recording: Arc::new(Mutex::new(None)),
            active_application: Arc::new(Mutex::new(None)),
            active_process_id: Arc::default(),
            capture_lock: Arc::default(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("DEVICE_BOT_ARTIFACT_DIR").unwrap_or_else(|_| "artifacts".into()))
    }

    fn ffmpeg_path() -> String {
        std::env::var("DEVICE_BOT_FFMPEG_PATH").unwrap_or_else(|_| {
            [
                "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
                "/usr/local/opt/ffmpeg/bin/ffmpeg",
            ]
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
            .map_or_else(|| "ffmpeg".into(), |path| (*path).into())
        })
    }

    fn screen_capture_device() -> String {
        std::env::var("DEVICE_BOT_MACOS_SCREEN_DEVICE")
            .unwrap_or_else(|_| "Capture screen 0".into())
    }

    fn require_device(&self, device_id: &str) -> Result<(), RobotError> {
        if device_id != DEVICE_ID {
            return Err(RobotError::DeviceNotFound(device_id.into()));
        }
        if !cfg!(target_os = "macos") {
            return Err(RobotError::Unsupported(
                "the macOS robot is available only when the MCP server runs on macOS".into(),
            ));
        }
        Ok(())
    }

    async fn run(&self, program: &str, args: &[String]) -> Result<Vec<u8>, RobotError> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not run {program}: {error}"))
            })?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RobotError::CommandFailed(format!(
                "{program} exited with {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".into(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    async fn apple_script(&self, script: &str, values: &[String]) -> Result<(), RobotError> {
        let mut args = vec!["-e".into(), script.into(), "--".into()];
        args.extend(values.iter().cloned());
        self.run(&self.osascript_path, &args).await.map(|_| ())
    }

    async fn accessibility_elements(
        &self,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        #[cfg(target_os = "macos")]
        {
            let filter = filter.map(str::to_owned).map(|value| value.to_lowercase());
            let application = *self.active_process_id.lock().await;
            tokio::time::timeout(
                Duration::from_millis(2_500),
                tokio::task::spawn_blocking(move || {
                    native_accessibility_elements(filter.as_deref(), max_results, application)
                }),
            )
            .await
            .map_err(|_| {
                RobotError::CommandFailed(
                    "macOS native accessibility filter exceeded its 2.5 second deadline".into(),
                )
            })?
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "macOS accessibility traversal task failed: {error}"
                ))
            })?
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (filter, max_results);
            Err(RobotError::Unsupported(
                "the macOS robot is available only when the MCP server runs on macOS".into(),
            ))
        }
    }

    async fn accessibility_tap(&self, point: Point) -> Result<(), RobotError> {
        #[cfg(target_os = "macos")]
        {
            let application = *self.active_process_id.lock().await;
            native_accessibility_tap(point, application)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // AXPress targets the smallest actionable element under the coordinate.
            // It is more reliable than a synthetic desktop click for Chrome's tab
            // strip and address field, while preserving coordinate-based MCP input.
            self.apple_script(
            r#"on run argv
set targetX to item 2 of argv as real
set targetY to item 3 of argv as real
set targetApp to item 1 of argv
tell application "System Events"
    if targetApp is "" then
        set frontProcess to first application process whose frontmost is true
    else
        set frontProcess to application process targetApp
    end if
    tell frontProcess
        set bestCandidate to missing value
        set bestArea to 0
        try
            set candidates to entire contents of front window
        on error
            set candidates to {}
        end try
        repeat with candidate in candidates
            try
                set {x, y} to position of candidate
                set {w, h} to size of candidate
                set candidateRole to role of candidate as text
                set candidateArea to w * h
                if targetX ≥ x and targetX ≤ (x + w) and targetY ≥ y and targetY ≤ (y + h) and candidateRole is in {"AXButton", "AXLink", "AXTextField", "AXRadioButton", "AXPopUpButton", "AXMenuItem"} then
                    if bestArea is 0 or candidateArea < bestArea then
                        set bestCandidate to candidate
                        set bestArea to candidateArea
                    end if
                end if
            end try
        end repeat
        if bestCandidate is not missing value then
            try
                perform action "AXPress" of bestCandidate
            on error
                click bestCandidate
            end try
        else
            click at {targetX, targetY}
        end if
    end tell
end tell
end run"#,
            &[
                self.active_application.lock().await.clone().unwrap_or_default(),
                point.x.round().to_string(),
                point.y.round().to_string(),
            ],
        )
        .await
        }
    }
}

#[cfg(test)]
fn parse_accessibility_elements(output: &str) -> Vec<UiElement> {
    output
        .trim()
        .split('\x1e')
        .filter_map(|line| {
            let fields = line.split('\x1f').collect::<Vec<_>>();
            if fields.len() != 8 {
                return None;
            }
            let x = fields[4].trim().parse::<f64>().ok()?;
            let y = fields[5].trim().parse::<f64>().ok()?;
            let width = fields[6].trim().parse::<f64>().ok()?;
            let height = fields[7].trim().parse::<f64>().ok()?;
            let role = fields[0].trim();
            let title = non_empty(fields[1]);
            let description = non_empty(fields[2]);
            let value = non_empty(fields[3]);
            Some(UiElement {
                class_name: if role.is_empty() {
                    "AXUnknown".into()
                } else {
                    role.into()
                },
                resource_id: None,
                text: title.or_else(|| value.clone()),
                content_description: description,
                bounds: robot_core::Bounds {
                    left: x.round() as i32,
                    top: y.round() as i32,
                    right: (x + width).round() as i32,
                    bottom: (y + height).round() as i32,
                },
                clickable: matches!(role, "AXButton" | "AXLink" | "AXMenuItem" | "AXPopUpButton"),
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value,
                element_type: None,
            })
        })
        .collect()
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.into())
}

#[async_trait]
impl Robot for MacosRobot {
    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        if !cfg!(target_os = "macos") {
            return Ok(vec![]);
        }
        Ok(vec![RobotDevice {
            id: DEVICE_ID.into(),
            name: "This Mac".into(),
            platform: Platform::Macos,
            state: DeviceState::Available,
        }])
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        self.require_device(device_id)?;
        let output = self
            .run(
                "system_profiler",
                &["SPApplicationsDataType".into(), "-json".into()],
            )
            .await?;
        let report = serde_json::from_slice::<Value>(&output).map_err(|error| {
            RobotError::CommandFailed(format!("system_profiler returned invalid JSON: {error}"))
        })?;
        let apps = report
            .get("SPApplicationsDataType")
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(json!({
            "platform": "macos",
            "deviceId": device_id,
            "apps": apps,
        }))
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        self.capture_desktop(&robot_core::ScreenshotOptions::default())
            .await
    }

    async fn take_screenshot_with_options(
        &self,
        device_id: &str,
        options: &robot_core::ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        self.require_device(device_id)?;
        self.capture_desktop(options).await
    }

    async fn list_displays(
        &self,
        device_id: &str,
    ) -> Result<Vec<robot_core::DesktopDisplay>, RobotError> {
        self.require_device(device_id)?;
        self.desktop_displays().await
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        if !request_screen_recording_permission() {
            return Err(RobotError::CommandFailed(
                "Screen Recording permission is required. macOS has been asked to grant it; approve the prompt, then retry the recording.".into(),
            ));
        }
        let mut recording = self.recording.lock().await;
        if recording.is_some() {
            return Err(RobotError::InvalidInput(
                "a macOS screen recording is already in progress".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not create recording directory: {error}"))
            })?;
        let path = self.artifact_dir.join(format!(
            "macos-recording-{}.mp4",
            RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // Select AVFoundation's named desktop source, never its numeric device
        // index: indexes can point at a connected Continuity camera. Unlike
        // `screencapture -v`, ffmpeg finalizes a valid MP4 on SIGINT.
        let input = format!("{}:none", Self::screen_capture_device());
        let child = Command::new(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "avfoundation",
                "-capture_cursor",
                "1",
                "-framerate",
                "30",
                "-i",
            ])
            .arg(input)
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not start ffmpeg screen recording (run --install-chrome-recording-tools or set DEVICE_BOT_FFMPEG_PATH): {error}"
                ))
            })?;
        *recording = Some(ActiveRecording {
            path: path.clone(),
            child,
        });
        Ok(path.to_string_lossy().into_owned())
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.require_device(device_id)?;
        let Some(recording) = self.recording.lock().await.take() else {
            return Err(RobotError::InvalidInput(
                "no macOS screen recording is in progress".into(),
            ));
        };
        #[cfg(target_os = "macos")]
        if let Some(pid) = recording.child.id() {
            // ffmpeg finalizes a playable MP4 when interrupted with SIGINT.
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let recording = {
            let mut recording = recording;
            recording.child.start_kill().map_err(|error| {
                RobotError::CommandFailed(format!("could not stop screen recording: {error}"))
            })?;
            recording
        };
        let output = recording.child.wait_with_output().await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not finalize macOS screen recording: {error}"
            ))
        })?;
        if !recording.path.is_file() {
            return Err(RobotError::CommandFailed(format!(
                "macOS did not create a screen-recording artifact (ffmpeg status {}): {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(recording.path.to_string_lossy().into_owned())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.accessibility_tap(point).await
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let mut launch_args = vec!["-a".into(), app_id.into()];
        if let Ok(arguments) = std::env::var("DEVICE_BOT_MACOS_APP_ARGS") {
            let arguments = arguments.split_whitespace().collect::<Vec<_>>();
            if !arguments.is_empty() {
                // `open -a` ignores `--args` when the application is already
                // running. Start a fresh instance so flags such as Chrome's
                // `--force-renderer-accessibility` take effect.
                launch_args.insert(0, "-n".into());
                launch_args.push("--args".into());
                launch_args.extend(arguments.into_iter().map(str::to_owned));
            }
        }
        self.run(&self.open_path, &launch_args).await?;
        let mut last_error = None;
        for _ in 0..20 {
            match self
                .apple_script(
                    "on run argv\n tell application (item 1 of argv) to activate\nend run",
                    &[app_id.into()],
                )
                .await
            {
                Ok(()) => {
                    *self.active_application.lock().await = Some(app_id.into());
                    *self.active_process_id.lock().await =
                        Some(self.focus_command("frontmost", "").await?);
                    return Ok(());
                }
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Err(last_error
            .unwrap_or_else(|| RobotError::CommandFailed(format!("could not activate {app_id}"))))
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        let _ = (url, app_id);
        Err(RobotError::Unsupported(
            "URL navigation belongs to the Chrome DevTools robot; use a chrome:<target-id> device"
                .into(),
        ))
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        self.apple_script(
            "on run argv\n set targetApp to item 1 of argv\n set textToType to item 2 of argv\n if targetApp is not \"\" then tell application targetApp to activate\n tell application \"System Events\"\n  if textToType is character id 27 then\n   key code 53\n  else\n   set AppleScript's text item delimiters to linefeed\n   set parts to text items of textToType\n   repeat with index from 1 to count of parts\n    keystroke (item index of parts)\n    if index is less than count of parts then key code 36\n   end repeat\n  end if\n end tell\nend run",
            &[
                self.active_application.lock().await.clone().unwrap_or_default(),
                text.into(),
            ],
        ).await
    }

    async fn swipe(
        &self,
        device_id: &str,
        _start: Point,
        _end: Point,
        _duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "macOS drag gestures require a pointer automation backend; tap and keyboard input are available through System Events".into(),
        ))
    }

    async fn install_app(&self, device_id: &str, _apk_path: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app installation is not supported by the macOS robot".into(),
        ))
    }

    async fn uninstall_app(&self, device_id: &str, _app_id: &str) -> Result<(), RobotError> {
        self.require_device(device_id)?;
        Err(RobotError::Unsupported(
            "app removal is not supported by the macOS robot".into(),
        ))
    }

    async fn list_flutter_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        if let Some(elements) = robot_flutter_bridge::configured(device_id).await {
            return Ok(elements);
        }
        #[cfg(target_os = "macos")]
        {
            let pid = {
                let application =
                    native_frontmost_application(*self.active_process_id.lock().await)?;
                let mut pid: i32 = 0;
                let status = unsafe { AXUIElementGetPid(application.0, &mut pid) };
                if status != 0 || pid <= 0 {
                    return Ok(vec![]);
                }
                pid as u32
            };
            return Ok(robot_flutter_bridge::for_process(pid)
                .await
                .unwrap_or_default());
        }
        #[cfg(not(target_os = "macos"))]
        Ok(vec![])
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        self.accessibility_elements(None, None).await
    }

    async fn list_elements_filtered(
        &self,
        device_id: &str,
        filter: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        self.require_device(device_id)?;
        // Filter during native AXUIElement traversal in the MCP process, and
        // stop descending once the requested result count has been reached.
        self.accessibility_elements(filter, max_results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_device_ids() {
        assert!(
            MacosRobot::new("artifacts")
                .require_device("another-device")
                .is_err()
        );
    }

    #[test]
    fn parses_system_events_elements() {
        let elements = parse_accessibility_elements(
            "AXButton\x1fNew Tab\x1fOpen a new tab\x1f\x1f12\x1f34\x1f24\x1f24",
        );
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].text.as_deref(), Some("New Tab"));
        assert_eq!(elements[0].bounds.right, 36);
        assert!(elements[0].clickable);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod screenshot_tests;

mod screenshot;

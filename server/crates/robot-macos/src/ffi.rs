//! CoreFoundation, Quartz, and Accessibility bindings with owned reference cleanup.

use std::{
    ffi::{c_char, c_void},
    os::raw::c_long,
};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(super) fn CGPreflightScreenCaptureAccess() -> bool;
    pub(super) fn CGRequestScreenCaptureAccess() -> bool;
    pub(super) fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

pub(super) type CFTypeRef = *const c_void;
pub(super) type CFStringRef = *const c_void;
pub(super) type CFArrayRef = *const c_void;
pub(super) type CFDictionaryRef = *const c_void;
pub(super) type AXUIElementRef = *const c_void;

pub(super) const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub(super) const K_AX_VALUE_CG_POINT_TYPE: u32 = 1;
pub(super) const K_AX_VALUE_CG_SIZE_TYPE: u32 = 2;
pub(super) const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
pub(super) const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
pub(super) const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CGPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[repr(C)]
pub(super) struct CGSize {
    pub(super) width: f64,
    pub(super) height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub(super) fn AXIsProcessTrusted() -> bool;
    pub(super) fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    pub(super) fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub(super) fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub(super) fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
    pub(super) fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    pub(super) fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    pub(super) fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> i32;
    pub(super) fn AXValueGetValue(value: CFTypeRef, value_type: u32, result: *mut c_void) -> bool;
    pub(super) static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(super) static kCFBooleanTrue: CFTypeRef;
    pub(super) fn CFRelease(value: CFTypeRef);
    pub(super) fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    pub(super) fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: c_long,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFDictionaryRef;
    pub(super) fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub(super) fn CFArrayGetTypeID() -> usize;
    pub(super) fn CFArrayGetCount(array: CFArrayRef) -> c_long;
    pub(super) fn CFArrayGetValueAtIndex(array: CFArrayRef, index: c_long) -> CFTypeRef;
    pub(super) fn CFGetTypeID(value: CFTypeRef) -> usize;
    pub(super) fn CFBooleanGetTypeID() -> usize;
    pub(super) fn CFBooleanGetValue(value: CFTypeRef) -> bool;
    pub(super) fn CFNumberGetValue(number: CFTypeRef, number_type: i32, value: *mut c_void)
    -> bool;
    pub(super) fn CFStringGetTypeID() -> usize;
    pub(super) fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: c_long,
        encoding: u32,
        is_external_representation: bool,
    ) -> CFStringRef;
    pub(super) fn CFStringGetLength(string: CFStringRef) -> c_long;
    pub(super) fn CFStringGetMaximumSizeForEncoding(length: c_long, encoding: u32) -> c_long;
    pub(super) fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: c_long,
        encoding: u32,
    ) -> bool;
}

pub(super) struct CfOwned(pub(super) CFTypeRef);

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

//! Desktop window discovery and portable projections into app/window contracts.

#[cfg(target_os = "macos")]
use super::accessibility::{native_cf_string, native_i32, native_string};
#[cfg(target_os = "macos")]
use super::ffi::{
    CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFDictionaryGetValue, CFGetTypeID,
    CGWindowListCopyWindowInfo, CfOwned, K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
    K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY,
};
#[cfg(target_os = "macos")]
use robot_core::RobotError;
use robot_core::{AppWindow, RunningApp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesktopWindow {
    pub(super) process_id: u32,
    pub(super) window_id: String,
    pub(super) owner: String,
    pub(super) title: String,
}

pub(super) fn running_apps_from_windows(windows: &[DesktopWindow]) -> Vec<RunningApp> {
    let mut apps = Vec::new();
    for window in windows {
        if apps
            .iter()
            .any(|app: &RunningApp| app.process_id == window.process_id)
        {
            continue;
        }
        apps.push(RunningApp {
            process_id: window.process_id,
            name: window.owner.clone(),
            bundle_id: None,
        });
    }
    apps.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.process_id.cmp(&right.process_id))
    });
    apps
}

pub(super) fn windows_for_process(
    windows: &[DesktopWindow],
    process_id: Option<u32>,
) -> Vec<AppWindow> {
    windows
        .iter()
        .filter(|window| process_id.is_none_or(|id| window.process_id == id))
        .map(|window| AppWindow {
            window_id: window.window_id.clone(),
            process_id: window.process_id,
            title: if window.title.is_empty() {
                window.owner.clone()
            } else {
                window.title.clone()
            },
            focused: false,
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn native_listed_windows() -> Result<Vec<DesktopWindow>, RobotError> {
    let windows = CfOwned(unsafe {
        CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        )
    });
    if windows.0.is_null() || unsafe { CFGetTypeID(windows.0) } != unsafe { CFArrayGetTypeID() } {
        return Err(RobotError::CommandFailed(
            "could not enumerate macOS windows".into(),
        ));
    }
    let layer_key = native_cf_string("kCGWindowLayer")?;
    let owner_pid_key = native_cf_string("kCGWindowOwnerPID")?;
    let owner_name_key = native_cf_string("kCGWindowOwnerName")?;
    let title_key = native_cf_string("kCGWindowName")?;
    let number_key = native_cf_string("kCGWindowNumber")?;
    let count = unsafe { CFArrayGetCount(windows.0) };
    let mut listed = Vec::new();
    for index in 0..count {
        let window = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
        if window.is_null() {
            continue;
        }
        if native_i32(unsafe { CFDictionaryGetValue(window, layer_key) }) != Some(0) {
            continue;
        }
        let Some(pid) = native_i32(unsafe { CFDictionaryGetValue(window, owner_pid_key) }) else {
            continue;
        };
        if pid <= 0 {
            continue;
        }
        let Some(number) = native_i32(unsafe { CFDictionaryGetValue(window, number_key) }) else {
            continue;
        };
        listed.push(DesktopWindow {
            process_id: pid as u32,
            window_id: number.to_string(),
            owner: native_string(unsafe { CFDictionaryGetValue(window, owner_name_key) })
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("pid {pid}")),
            title: native_string(unsafe { CFDictionaryGetValue(window, title_key) })
                .unwrap_or_default(),
        });
    }
    Ok(listed)
}

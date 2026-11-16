use serde::{Deserialize, Serialize};
use std::{
    fs, io, mem,
    os::windows::io::AsRawHandle,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForInputIdle,
            WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, SW_RESTORE,
        SetForegroundWindow, ShowWindow, WM_CLOSE,
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    process_id: u32,
    start_ticks: u64,
    executable: String,
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_ascii_lowercase()
}

fn identity(handle: HANDLE) -> io::Result<(u64, String)> {
    // SAFETY: the caller holds a live process handle, and all output buffers
    // remain valid for the duration of each synchronous Win32 call.
    unsafe {
        let mut created: FILETIME = mem::zeroed();
        let mut exited: FILETIME = mem::zeroed();
        let mut kernel: FILETIME = mem::zeroed();
        let mut user: FILETIME = mem::zeroed();
        if GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) == 0 {
            return Err(io::Error::last_os_error());
        }
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        let mut path = vec![0u16; 32_768];
        let mut length = path.len() as u32;
        if QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            ticks,
            path_key(Path::new(&String::from_utf16_lossy(
                &path[..length as usize],
            ))),
        ))
    }
}

fn existing(executable: &str) -> io::Result<bool> {
    // SAFETY: snapshots and opened process handles are closed by Handle. The
    // process-entry structure advertises its initialized size to the API.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Handle(snapshot);
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Process32FirstW(snapshot.0, &mut entry);
        while found != 0 {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
            if !process.is_null() {
                let process = Handle(process);
                if identity(process.0).is_ok_and(|(_, path)| path == executable) {
                    return Ok(true);
                }
            }
            found = Process32NextW(snapshot.0, &mut entry);
        }
        Ok(false)
    }
}

struct WindowSearch {
    pid: u32,
    windows: Vec<HWND>,
}
unsafe extern "system" fn collect(window: HWND, context: LPARAM) -> i32 {
    // SAFETY: EnumWindows invokes this synchronously with our stack-owned search.
    unsafe {
        let search = &mut *(context as *mut WindowSearch);
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == search.pid {
            search.windows.push(window);
        }
    }
    1
}
fn windows(pid: u32) -> io::Result<Vec<HWND>> {
    let mut search = WindowSearch {
        pid,
        windows: Vec::new(),
    };
    // SAFETY: callback context remains live throughout EnumWindows.
    if unsafe { EnumWindows(Some(collect), &mut search as *mut _ as LPARAM) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(search.windows)
}

pub(super) fn start(app: &Path, state_file: &Path) -> io::Result<()> {
    let app = fs::canonicalize(app)?;
    if state_file.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Windows fixture state already exists",
        ));
    }
    if existing(&path_key(&app))? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "close the existing app fixture before starting the project",
        ));
    }
    let mut child = Command::new(&app).spawn()?;
    let prepared = (|| {
        let (start_ticks, executable) = identity(child.as_raw_handle() as HANDLE)?;
        let state = State {
            process_id: child.id(),
            start_ticks,
            executable,
        };
        if let Some(parent) = state_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(state_file, serde_json::to_vec(&state)?)?;
        // SAFETY: Child owns this process handle. Non-GUI/early-exit cases are
        // diagnosed by the bounded readiness loop below.
        unsafe {
            WaitForInputIdle(child.as_raw_handle() as HANDLE, 10_000);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let registration = std::env::temp_dir()
            .join("device-bot-flutter")
            .join(format!("{}.json", child.id()));
        loop {
            if child.try_wait()?.is_some() {
                return Err(io::Error::other("Windows fixture exited during startup"));
            }
            for window in windows(child.id())? {
                let mut class = [0u16; 256];
                // SAFETY: class is a valid UTF-16 output buffer for this call.
                let length =
                    unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
                if length > 0
                    && String::from_utf16_lossy(&class[..length as usize])
                        == "FLUTTER_RUNNER_WIN32_WINDOW"
                    && registration.is_file()
                {
                    thread::sleep(Duration::from_secs(1));
                    // SAFETY: this window was enumerated for our exact child PID.
                    unsafe {
                        ShowWindow(window, SW_RESTORE);
                        ShowWindow(window, SW_RESTORE);
                    }
                    if unsafe { SetForegroundWindow(window) } == 0 {
                        return Err(io::Error::other(
                            "Windows fixture could not enter the foreground",
                        ));
                    }
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows fixture did not register its window and widget bridge within 10 seconds",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    })();
    if prepared.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(state_file);
    }
    prepared
}

pub(super) fn stop(app: &Path, state_file: &Path) -> io::Result<()> {
    let state: State = serde_json::from_slice(&fs::read(state_file)?)?;
    let expected = path_key(&fs::canonicalize(app)?);
    // SAFETY: OpenProcess returns a handle to one PID. Identity is checked on
    // that handle before any window close/termination, protecting against reuse.
    unsafe {
        let process = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            0,
            state.process_id,
        );
        if process.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(87) {
                fs::remove_file(state_file)?;
                return Ok(());
            }
            return Err(error);
        }
        let process = Handle(process);
        let (ticks, executable) = identity(process.0)?;
        if ticks == state.start_ticks && executable == state.executable && executable == expected {
            for window in windows(state.process_id)? {
                PostMessageW(window, WM_CLOSE, 0, 0);
            }
            match WaitForSingleObject(process.0, 2000) {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => {
                    if TerminateProcess(process.0, 1) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if WaitForSingleObject(process.0, 2000) != WAIT_OBJECT_0 {
                        return Err(io::Error::other("Windows fixture did not stop"));
                    }
                }
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }
    fs::remove_file(state_file)
}

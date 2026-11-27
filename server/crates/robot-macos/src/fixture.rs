//! Relaunch an explicitly bound, single-window fixture without targeting apps by name.

use super::{MacosRobot, live, windows::native_listed_windows};
use robot_core::RobotError;
use serde::{Deserialize, Serialize};
use std::{mem::MaybeUninit, path::PathBuf, time::Duration};

const APPKIT: &str = r#"ObjC.import('AppKit');
function run(argv) {
  function missing(value) { return !value || value.isNil(); }
  function running(pid) {
    return $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
  }
  function identity(app) {
    if (missing(app) || app.terminated || missing(app.bundleIdentifier) ||
        missing(app.bundleURL)) {
      throw new Error('The selected macOS fixture is not a running app bundle.');
    }
    return {
      pid: Number(app.processIdentifier),
      bundle: ObjC.unwrap(app.bundleIdentifier),
      path: ObjC.unwrap(app.bundleURL.path),
      launched: missing(app.launchDate) ? null : String(app.launchDate.timeIntervalSince1970)
    };
  }
  var app = running(Number(argv[1]));
  var old = identity(app);
  if (argv[0] === 'inspect') return JSON.stringify(old);
  var expected = JSON.parse(argv[2]);
  if (old.pid !== expected.pid || old.bundle !== expected.bundle ||
      old.path !== expected.path || old.launched !== expected.launched) {
    throw new Error('The selected macOS fixture process changed.');
  }
  if (!app.terminate) throw new Error('The selected macOS fixture did not accept quit.');
  var deadline = Date.now() + 3500;
  while (!app.terminated) {
    if (Date.now() >= deadline) throw new Error('The selected macOS fixture did not quit.');
    $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.05));
  }
  var url = $.NSURL.fileURLWithPath($(old.path));
  var workspace = $.NSWorkspace.sharedWorkspace;
  // NSWorkspaceLaunchNewInstance avoids reusing a separate instance with the same bundle ID.
  var reopened = workspace.launchApplicationAtURLOptionsConfigurationError(url, 0x00080000, $({}), null);
  var next = identity(reopened);
  if (next.pid === old.pid || next.bundle !== old.bundle || next.path !== old.path) {
    throw new Error('macOS did not return a new instance of the selected fixture bundle.');
  }
  return JSON.stringify(next);
}"#;

#[derive(Debug, Deserialize, Serialize)]
struct Identity {
    pid: i32,
    bundle: String,
    path: PathBuf,
    launched: Option<String>,
}

#[derive(Debug)]
pub(super) struct Binding {
    id: String,
    process: Identity,
    start: (u64, u64),
}

// LaunchServices has no launch date for apps started directly (e.g. flutter run).
// Keep the kernel's process generation as well as the AppKit bundle identity.
fn process_start(pid: i32) -> Result<(u64, u64), RobotError> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if bytes != size {
        return Err(RobotError::InvalidInput(
            "Could not verify the selected macOS process generation".into(),
        ));
    }
    // proc_pidinfo filled the entire proc_bsdinfo buffer.
    let info = unsafe { info.assume_init() };
    Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
}

fn valid_bundle(bundle: &str) -> bool {
    !matches!(bundle, "" | "." | "..")
        && bundle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn single_window(pid: i32) -> Result<Option<String>, RobotError> {
    let mut windows = native_listed_windows()?
        .into_iter()
        .filter(|window| window.process_id == pid as u32);
    let first = windows.next();
    if windows.next().is_some() {
        return Err(RobotError::InvalidInput(
            "macOS fixture reset requires exactly one visible window".into(),
        ));
    }
    Ok(first.map(|window| format!("macos:{pid}:{}", window.window_id)))
}

fn verify_window(id: &str) -> Result<live::Target, RobotError> {
    let target = live::Target::parse(id)?.ok_or_else(|| {
        RobotError::InvalidInput("Select the macOS fixture's exact window first".into())
    })?;
    live::verify_single_fixture_window(target)?;
    Ok(target)
}

impl MacosRobot {
    async fn fixture_appkit(&self, args: Vec<String>) -> Result<Identity, RobotError> {
        let command = ["-l", "JavaScript", "-e", APPKIT, "--"]
            .into_iter()
            .map(str::to_owned)
            .chain(args)
            .collect::<Vec<_>>();
        let bytes = tokio::time::timeout(
            Duration::from_secs(8),
            self.run(&self.osascript_path, &command),
        )
        .await
        .map_err(|_| {
            RobotError::CommandFailed("macOS fixture lifecycle command timed out".into())
        })??;
        serde_json::from_slice(&bytes).map_err(|_| {
            RobotError::CommandFailed(
                "Could not verify the macOS fixture's process identity".into(),
            )
        })
    }

    pub(super) async fn bind_fixture(&self, id: &str, bundle: &str) -> Result<(), RobotError> {
        let _guard = self.capture_lock.lock().await;
        let target = verify_window(id)?;
        if !valid_bundle(bundle) || single_window(target.pid)?.as_deref() != Some(id) {
            return Err(RobotError::InvalidInput(
                "Expected a bundle ID and the selected fixture's only visible window".into(),
            ));
        }
        let start = process_start(target.pid)?;
        let process = self
            .fixture_appkit(vec!["inspect".into(), target.pid.to_string()])
            .await?;
        if process.bundle != bundle || process.pid != target.pid || !process.path.is_absolute() {
            return Err(RobotError::InvalidInput(
                "resetAppId must match the bundle ID of the selected macOS window".into(),
            ));
        }
        if process_start(target.pid)? != start {
            return Err(RobotError::InvalidInput(
                "The selected macOS fixture process changed".into(),
            ));
        }
        *self.fixture.lock().await = Some(Binding {
            id: id.into(),
            process,
            start,
        });
        Ok(())
    }

    pub(super) async fn restart_fixture(&self, id: &str) -> Result<String, RobotError> {
        let _guard = self.capture_lock.lock().await;
        let mut binding = self.fixture.lock().await;
        let old = binding.as_ref().filter(|old| old.id == id).ok_or_else(|| {
            RobotError::InvalidInput(
                "The selected macOS fixture is not bound to this window".into(),
            )
        })?;
        let target = verify_window(id)?;
        if single_window(target.pid)?.as_deref() != Some(id)
            || process_start(target.pid)? != old.start
        {
            return Err(RobotError::InvalidInput(
                "The selected fixture window changed".into(),
            ));
        }
        let expected = serde_json::to_string(&old.process).map_err(|_| {
            RobotError::InvalidInput("The macOS fixture identity could not be encoded".into())
        })?;
        // If quit or launch fails, don't keep a binding to a potentially closed window.
        *binding = None;
        let process = self
            .fixture_appkit(vec!["reopen".into(), target.pid.to_string(), expected])
            .await?;
        let start = process_start(process.pid)?;
        let ready_by = tokio::time::Instant::now() + Duration::from_secs(5);
        let new_id = loop {
            if let Some(id) = single_window(process.pid)?
                && verify_window(&id).is_ok()
            {
                break id;
            }
            if tokio::time::Instant::now() >= ready_by {
                return Err(RobotError::CommandFailed(
                    "The relaunched macOS fixture did not expose a verifiable window".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(125)).await;
        };
        if process_start(process.pid)? != start {
            return Err(RobotError::InvalidInput(
                "The relaunched macOS fixture process changed".into(),
            ));
        }
        self.live_geometry.lock().await.remove(id);
        self.flutter_windows.lock().await.remove(id);
        *binding = Some(Binding {
            id: new_id.clone(),
            process,
            start,
        });
        Ok(new_id)
    }

    pub(super) async fn fixture_bridge_directory(&self, id: &str) -> Option<PathBuf> {
        let binding = self.fixture.lock().await;
        let process = &binding.as_ref().filter(|binding| binding.id == id)?.process;
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Containers")
                .join(&process.bundle)
                .join("Data/tmp/device-bot-flutter"),
        )
    }
}

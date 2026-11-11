use super::*;
use robot_core::{DesktopDisplay, ScreenshotOptions};

// AppKit focus control does not send Apple Events to System Events or the target app.
const FOCUS: &str = r#"ObjC.import('AppKit');
function run(argv) {
  var workspace = $.NSWorkspace.sharedWorkspace;
  function missing(value) { return !value || value.isNil(); }
  function frontPid() {
    var app = workspace.frontmostApplication;
    if (missing(app)) throw new Error('No foreground macOS application.');
    return Number(app.processIdentifier);
  }
  function waitFor(pid, message) {
    for (var i = 0; i < 60; i++) {
      if (frontPid() === pid) return String(pid);
      $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.05));
    }
    throw new Error(message);
  }
  if (argv[0] === 'frontmost') return String(frontPid());
  var app, pid;
  if (argv[0] === 'restore') {
    pid = Number(argv[1]);
    // Some menu-bar apps cannot be activated; leave them alone when already foreground.
    if (frontPid() === pid) return String(pid);
    app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
    if (missing(app)) throw new Error('Previous foreground application has exited.');
  } else if (argv[0] === 'activate') {
    var target = String(argv[1]);
    function matches(candidate) {
      return ObjC.unwrap(candidate.bundleIdentifier) === target ||
          ObjC.unwrap(candidate.localizedName) === target ||
          (!missing(candidate.bundleURL) && ObjC.unwrap(candidate.bundleURL.path) === target);
    }
    // Multiple instances may share a bundle ID (for example Chrome or VNC).
    var foreground = workspace.frontmostApplication;
    if (!missing(foreground) && matches(foreground)) return String(foreground.processIdentifier);
    var running = workspace.runningApplications;
    for (var j = 0; j < running.count; j++) {
      var candidate = running.objectAtIndex(j);
      if (matches(candidate)) {
        app = candidate;
        break;
      }
    }
    if (missing(app)) {
      var url = workspace.URLForApplicationWithBundleIdentifier($(target));
      if (missing(url)) {
        var path = target.charAt(0) === '/' ? $(target) : workspace.fullPathForApplication($(target));
        if (missing(path)) throw new Error('Could not find target application: ' + target);
        url = $.NSURL.fileURLWithPath(path);
      }
      app = workspace.launchApplicationAtURLOptionsConfigurationError(url, 0, $({}), null);
      if (missing(app)) throw new Error('Could not launch target application: ' + target);
    }
    pid = Number(app.processIdentifier);
  } else {
    throw new Error('Unsupported focus operation: ' + argv[0]);
  }
  if (frontPid() === pid) return String(pid);
  app.activateWithOptions(3);
  return waitFor(pid, argv[0] === 'restore'
    ? 'Could not restore the previous foreground app.'
    : 'Could not focus target app; screenshot was not taken.');
}"#;

const DISPLAYS: &str = r#"ObjC.import('AppKit');
function run() {
  var screens = $.NSScreen.screens, result = [];
  var primaryHeight = screens.objectAtIndex(0).frame.size.height;
  for (var i=0; i<screens.count; i++) {
    var screen = screens.objectAtIndex(i), frame = screen.frame;
    var left = Math.round(frame.origin.x);
    var top = Math.round(primaryHeight - frame.origin.y - frame.size.height);
    result.push({id:String(ObjC.unwrap(screen.deviceDescription.objectForKey('NSScreenNumber'))),
      name:'Screen '+(i+1)+' — '+ObjC.unwrap(screen.localizedName), isPrimary:i===0,
      bounds:{left:left,top:top,right:left+Math.round(frame.size.width),bottom:top+Math.round(frame.size.height)}});
  }
  return JSON.stringify(result);
}"#;

impl MacosRobot {
    pub(super) async fn focus_command(&self, mode: &str, value: &str) -> Result<i32, RobotError> {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            self.run(
                &self.osascript_path,
                &[
                    "-l".into(),
                    "JavaScript".into(),
                    "-e".into(),
                    FOCUS.into(),
                    "--".into(),
                    mode.into(),
                    value.into(),
                ],
            ),
        )
        .await
        .map_err(|_| RobotError::CommandFailed(format!("macOS focus {mode} timed out")))??;
        String::from_utf8_lossy(&output)
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| {
                RobotError::CommandFailed("macOS focus did not return a valid process id".into())
            })
    }

    pub(super) async fn desktop_displays(&self) -> Result<Vec<DesktopDisplay>, RobotError> {
        let output = self
            .run(
                &self.osascript_path,
                &[
                    "-l".into(),
                    "JavaScript".into(),
                    "-e".into(),
                    DISPLAYS.into(),
                ],
            )
            .await?;
        let displays: Vec<DesktopDisplay> = serde_json::from_slice(&output).map_err(|error| {
            RobotError::CommandFailed(format!("invalid macOS display list: {error}"))
        })?;
        if displays.is_empty() {
            return Err(RobotError::CommandFailed("no active macOS displays".into()));
        }
        Ok(displays)
    }

    pub(super) async fn capture_desktop(
        &self,
        options: &ScreenshotOptions,
    ) -> Result<Vec<u8>, RobotError> {
        let _lock = self.capture_lock.lock().await;
        let mut args = vec!["-x".into()];
        if let Some(id) = &options.display_id {
            let display = self
                .desktop_displays()
                .await?
                .into_iter()
                .find(|d| d.id == *id)
                .ok_or_else(|| {
                    RobotError::InvalidInput(format!(
                        "display {id} is no longer connected; refresh displays"
                    ))
                })?;
            let b = display.bounds;
            args.push(format!(
                "-R{},{},{},{}",
                b.left,
                b.top,
                b.right - b.left,
                b.bottom - b.top
            ));
        } else {
            args.push("-m".into());
        }
        let app = options
            .app_id
            .clone()
            .or(self.active_application.lock().await.clone());
        let previous = if app.is_some() {
            Some(self.focus_command("frontmost", "").await?)
        } else {
            None
        };
        let path = self.artifact_dir.join(format!(
            "macos-screenshot-{}.png",
            SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = async {
            if let Some(app) = &app {
                let pid = self.focus_command("activate", app).await?;
                *self.active_application.lock().await = Some(app.clone());
                *self.active_process_id.lock().await = Some(pid);
            }
            tokio::fs::create_dir_all(&self.artifact_dir)
                .await
                .map_err(|error| {
                    RobotError::CommandFailed(format!(
                        "could not create screenshot directory: {error}"
                    ))
                })?;
            args.push(path.to_string_lossy().into_owned());
            self.run(&self.screencapture_path, &args).await?;
            tokio::fs::read(&path).await.map_err(|error| {
                RobotError::CommandFailed(format!("could not read macOS screenshot: {error}"))
            })
        }
        .await;
        let _ = tokio::fs::remove_file(path).await;
        let restore = match previous {
            Some(pid) => self
                .focus_command("restore", &pid.to_string())
                .await
                .map(|_| ()),
            None => Ok(()),
        };
        match (result, restore) {
            (Ok(image), Ok(())) => Ok(image),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(RobotError::CommandFailed(format!(
                "screenshot captured, but focus restoration failed: {error}"
            ))),
            (Err(error), Err(restore)) => Err(RobotError::CommandFailed(format!(
                "{error}; focus restoration also failed: {restore}"
            ))),
        }
    }
}

use super::*;
use robot_core::{DesktopDisplay, ScreenshotOptions};

const FOCUS: &str = r#"on run argv
set mode to item 1 of argv
if mode is "activate" then
    set targetApp to item 2 of argv
    tell application targetApp to activate
    set targetId to id of application targetApp
    repeat 60 times
        tell application "System Events"
            set frontProcess to first application process whose frontmost is true
            if bundle identifier of frontProcess is targetId then return unix id of frontProcess
        end tell
        delay 0.05
    end repeat
    error "Could not focus target app; screenshot was not taken."
else if mode is "restore" then
    set targetPid to item 2 of argv as integer
    tell application "System Events"
        set targetProcess to first application process whose unix id is targetPid
        set frontmost of targetProcess to true
    end tell
    repeat 60 times
        tell application "System Events"
            if unix id of first application process whose frontmost is true is targetPid then return targetPid
        end tell
        delay 0.05
    end repeat
    error "Could not restore the previous foreground app."
else
    tell application "System Events" to return unix id of first application process whose frontmost is true
end if
end run"#;

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
            Duration::from_secs(8),
            self.run(
                &self.osascript_path,
                &[
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

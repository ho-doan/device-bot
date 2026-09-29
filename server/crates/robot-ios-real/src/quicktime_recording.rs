use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use mobile_robot_core::RobotError;
use tokio::{process::Command, time::sleep};

static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const QUICKTIME_START_SCRIPT: &str = r#"
on run argv
    set deviceHint to "iPhone"
    if (count of argv) > 0 then set deviceHint to item 1 of argv

    tell application "QuickTime Player"
        activate
        new movie recording
    end tell

    tell application "System Events"
        tell process "QuickTime Player"
            set recordingWindow to first window whose name is "Movie Recording"
            try
                set captureButton to first button of recordingWindow whose name is "show capture device selection menu"
            on error
                set captureButton to button 2 of recordingWindow
            end try
            click captureButton
            delay 0.5
            keystroke deviceHint
            key code 36
            delay 0.5
            try
                click first button of recordingWindow whose name is "start recording"
            on error
                click button 3 of recordingWindow
            end try
        end tell
    end tell
end run
"#;

const QUICKTIME_STOP_SCRIPT: &str = r#"
tell application "System Events"
    tell process "QuickTime Player"
        set recordingWindow to first window whose name is "Movie Recording"
        try
            click first button of recordingWindow whose name is "stop recording"
        on error
            click button 3 of recordingWindow
        end try
    end tell
end tell
"#;

const QUICKTIME_CLOSE_SCRIPT: &str = r#"
tell application "QuickTime Player"
    close front document saving no
end tell
"#;

#[derive(Debug)]
pub struct QuickTimeRecording {
    artifact_path: PathBuf,
    started_at: SystemTime,
}

impl QuickTimeRecording {
    pub fn path(&self) -> String {
        self.artifact_path.to_string_lossy().into_owned()
    }

    pub async fn start(artifact_dir: &Path, device_hint: &str) -> Result<Self, RobotError> {
        tokio::fs::create_dir_all(artifact_dir)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not create iOS real-device recording directory: {error}"
                ))
            })?;

        let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let artifact_path = artifact_dir.join(format!(
            "mobile-mcp-ios-real-recording-{}-{sequence}.mp4",
            std::process::id()
        ));
        let device_hint = std::env::var("DEVICE_BOT_IOS_QUICKTIME_DEVICE")
            .unwrap_or_else(|_| device_hint.to_owned());
        let started_at = SystemTime::now();
        run_osascript(QUICKTIME_START_SCRIPT, &[device_hint.as_str()])
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!("could not start QuickTime iPhone recording: {error}. Connect the trusted iPhone by USB, allow QuickTime automation, and set DEVICE_BOT_IOS_QUICKTIME_DEVICE if needed."))
            })?;
        Ok(Self {
            artifact_path,
            started_at,
        })
    }

    pub async fn stop(self) -> Result<String, RobotError> {
        run_osascript(QUICKTIME_STOP_SCRIPT, &[])
            .await
            .map_err(RobotError::CommandFailed)?;
        let source_path = wait_for_quicktime_movie(self.started_at).await?;
        convert_movie_to_mp4(&source_path, &self.artifact_path).await?;

        let metadata = tokio::fs::metadata(&self.artifact_path)
            .await
            .map_err(|error| {
                RobotError::CommandFailed(format!(
                    "QuickTime stopped, but did not create recording artifact {}: {error}",
                    self.artifact_path.display()
                ))
            })?;
        if metadata.len() == 0 {
            return Err(RobotError::CommandFailed(format!(
                "QuickTime created an empty recording artifact {}",
                self.artifact_path.display()
            )));
        }
        let _ = run_osascript(QUICKTIME_CLOSE_SCRIPT, &[]).await;
        Ok(self.artifact_path.to_string_lossy().into_owned())
    }
}

fn quicktime_autosave_dir() -> Result<PathBuf, RobotError> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        RobotError::CommandFailed("HOME is required to read the QuickTime recording".into())
    })?;
    Ok(
        home.join(
            "Library/Containers/com.apple.QuickTimePlayerX/Data/Library/Autosave Information",
        ),
    )
}

async fn wait_for_quicktime_movie(started_at: SystemTime) -> Result<PathBuf, RobotError> {
    let autosave_dir = quicktime_autosave_dir()?;
    let mut previous: Option<(PathBuf, u64)> = None;
    let mut stable_checks = 0;
    for _ in 0..240 {
        if let Some(path) = newest_movie(&autosave_dir, started_at).await? {
            let size = tokio::fs::metadata(&path)
                .await
                .map(|metadata| metadata.len())
                .unwrap_or_default();
            if previous.as_ref().is_some_and(|(old_path, old_size)| {
                old_path == &path && *old_size == size && size > 0
            }) {
                stable_checks += 1;
                if stable_checks >= 4 {
                    return Ok(path);
                }
            } else {
                previous = Some((path, size));
                stable_checks = 0;
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
    Err(RobotError::CommandFailed(format!(
        "QuickTime did not finish its autosave MOV under {}",
        autosave_dir.display()
    )))
}

async fn newest_movie(root: &Path, started_at: SystemTime) -> Result<Option<PathBuf>, RobotError> {
    let mut pending = vec![root.to_owned()];
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    while let Some(directory) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(RobotError::CommandFailed(format!(
                    "could not scan QuickTime autosave directory {}: {error}",
                    directory.display()
                )));
            }
        };
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not read QuickTime autosave directory {}: {error}",
                directory.display()
            ))
        })? {
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(|error| {
                RobotError::CommandFailed(format!(
                    "could not inspect QuickTime autosave path {}: {error}",
                    path.display()
                ))
            })?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name == "Movie Recording.mov")
                && metadata.len() > 0
            {
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                if modified < started_at
                    || newest
                        .as_ref()
                        .is_some_and(|(current, _)| modified <= *current)
                {
                    continue;
                }
                newest = Some((modified, path));
            }
        }
    }
    Ok(newest.map(|(_, path)| path))
}

async fn convert_movie_to_mp4(source: &Path, destination: &Path) -> Result<(), RobotError> {
    let ffmpeg_path = std::env::var("DEVICE_BOT_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into());
    let output = Command::new(&ffmpeg_path)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|error| {
            RobotError::CommandFailed(format!(
                "could not run ffmpeg for QuickTime recording (set DEVICE_BOT_FFMPEG_PATH if needed): {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(RobotError::CommandFailed(format!(
            "ffmpeg could not convert QuickTime MOV to MP4: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

async fn run_osascript(script: &str, arguments: &[&str]) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("QuickTime recording is only available on macOS".into());
    }
    let osascript_path =
        std::env::var("DEVICE_BOT_OSASCRIPT_PATH").unwrap_or_else(|_| "osascript".into());
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new(osascript_path)
            .args(["-e", script, "--"])
            .args(arguments)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "osascript timed out while controlling QuickTime".to_owned())?
    .map_err(|error| format!("could not run osascript: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().into());
    }
    Err(format!(
        "osascript exited with {}: {}",
        output
            .status
            .code()
            .map_or_else(|| "signal".into(), |code| code.to_string()),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finds_a_new_quicktime_autosave_movie() {
        let root = std::env::temp_dir().join(format!(
            "mobile-mcp-quicktime-autosave-test-{}",
            std::process::id()
        ));
        let movie_dir = root.join("Unsaved QuickTime Player Document.qtpxcomposition");
        tokio::fs::create_dir_all(&movie_dir).await.unwrap();
        let movie = movie_dir.join("Movie Recording.mov");
        let started_at = SystemTime::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        tokio::fs::write(&movie, b"test-movie").await.unwrap();

        assert_eq!(newest_movie(&root, started_at).await.unwrap(), Some(movie));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn quicktime_scripts_use_screen_source_and_ui_stop() {
        assert!(QUICKTIME_START_SCRIPT.contains("keystroke deviceHint"));
        assert!(QUICKTIME_STOP_SCRIPT.contains("stop recording"));
    }
}

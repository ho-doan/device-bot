use std::path::Path;

/// Media helpers must never create a Windows console or steal QC focus.
pub(super) fn command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

/// Desktop launchers may not inherit the terminal's PATH.
pub(super) fn resolve(
    override_path: Option<String>,
    executable: Option<&Path>,
    known_paths: &[&str],
) -> String {
    if let Some(path) = override_path.filter(|path| !path.trim().is_empty()) {
        return path;
    }
    if let Some(directory) = executable.and_then(Path::parent) {
        let name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        for path in [
            directory.join("ffmpeg").join("bin").join(name),
            directory.join(name),
        ] {
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    known_paths
        .iter()
        .find(|path| Path::new(path).is_file())
        .map_or_else(|| "ffmpeg".into(), |path| (*path).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn media_helper_is_not_attached_to_a_console() {
        let mut probe = command("powershell.exe");
        probe.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type 'using System; using System.Runtime.InteropServices; public class ConsoleProbe { [DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; [Console]::Write([ConsoleProbe]::GetConsoleWindow().ToInt64())",
        ]);
        let output = tokio::time::timeout(std::time::Duration::from_secs(30), probe.output())
            .await
            .expect("console probe timed out")
            .expect("console probe could not start");
        assert!(
            output.status.success(),
            "console probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
    }

    #[test]
    fn discovers_bundled_and_homebrew_encoders_without_path() {
        let root = std::env::temp_dir().join(format!("device-bot-ffmpeg-{}", std::process::id()));
        let bundled = root.join("ffmpeg/bin").join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"fixture").unwrap();
        let exe = root.join("device-bot");
        assert_eq!(resolve(None, Some(&exe), &[]), bundled.to_string_lossy());
        assert_eq!(
            resolve(Some("custom encoder".into()), Some(&exe), &[]),
            "custom encoder"
        );
        assert_eq!(
            resolve(None, None, &[bundled.to_str().unwrap()]),
            bundled.to_string_lossy()
        );
        assert_eq!(resolve(None, None, &[]), "ffmpeg");
        std::fs::remove_dir_all(root).unwrap();
    }
}

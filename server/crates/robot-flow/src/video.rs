//! Step captions rendered into the recorded artifact, independently of the app UI.
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::Command,
    time::{Instant, timeout},
};

#[derive(Debug)]
struct Cue {
    at: f64,
    id: String,
    action: String,
    label: String,
}

#[derive(Debug)]
pub(crate) struct VideoOverlay {
    started: Instant,
    duration: Option<f64>,
    cues: Vec<Cue>,
    recent_inputs: Vec<(String, String)>,
}

impl VideoOverlay {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            duration: None,
            cues: Vec::new(),
            recent_inputs: Vec::new(),
        }
    }

    pub(crate) fn mark(&mut self, id: &str, action: &str, args: &Value) {
        let mut label = format!(
            "STEP {}\n{}",
            if id.is_empty() { "(unnamed)" } else { id },
            action
        );
        if let Some(target) = args
            .get("target")
            .or(args.get("label"))
            .and_then(Value::as_str)
        {
            label.push_str(&format!(" | target: {target}"));
        }
        if let Some(input) = args
            .get("text")
            .or_else(|| args.pointer("/arguments/text"))
            .and_then(Value::as_str)
        {
            // Requested test evidence includes the actual input, even for sensitive steps.
            // This label is never included in report.json or the MCP trace.
            let field = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_owned();
            self.recent_inputs.retain(|(name, _)| name != &field);
            self.recent_inputs.push((field, input.to_owned()));
            if self.recent_inputs.len() > 3 {
                self.recent_inputs.remove(0);
            }
        } else if let Some(direction) = args.get("direction").and_then(Value::as_str) {
            label.push_str(&format!(" | direction: {direction}"));
        } else if action == "tap" {
            label.push_str(&format!(" | x: {}, y: {}", args["x"], args["y"]));
        }
        // Retain recent input across fast steps that may fall between video frames.
        for (field, input) in &self.recent_inputs {
            label.push_str(&format!("\ninput [{field}]: {input}"));
        }
        self.cues.push(Cue {
            at: self.started.elapsed().as_secs_f64(),
            id: id.into(),
            action: action.into(),
            label,
        });
    }

    pub(crate) fn finish(&mut self) {
        self.duration = Some(self.started.elapsed().as_secs_f64().max(0.001));
    }

    pub(crate) async fn render(&self, directory: &Path) -> Result<(), String> {
        let input = directory.join("video.mp4");
        if !input.is_file() || self.cues.is_empty() {
            return Ok(());
        }
        let ffmpeg = ffmpeg_path();
        let probe = ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let mut command = Command::new(probe);
        command
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height:format=duration",
                "-of",
                "json",
            ])
            .arg(&input);
        let metadata = output(command, 15)
            .await
            .map_err(|e| format!("video step overlay: ffprobe failed: {e}"))?;
        let metadata: Value = serde_json::from_slice(&metadata)
            .map_err(|_| "video step overlay: invalid ffprobe metadata")?;
        let duration = metadata["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.)
            .ok_or("video step overlay: invalid video duration")?;
        let width = metadata["streams"][0]["width"]
            .as_u64()
            .filter(|w| *w > 0 && *w <= 16384)
            .ok_or("video step overlay: missing video width")?;
        let font = (width / 64).clamp(16, 30);
        let columns = (width / font).max(20) as usize;
        let labels: Vec<_> = self
            .cues
            .iter()
            .map(|cue| wrap(&cue.label, columns))
            .collect();
        let rows = labels
            .iter()
            .map(|label| label.lines().count())
            .max()
            .unwrap_or(1);
        // Fonts with CJK coverage have larger line metrics than their pixel size.
        // Reserve enough height so the last input line stays on the dark header.
        let header = ((rows as u64 * (font * 3 / 2 + 8) + 24) + 1) / 2 * 2;
        let work = directory.join(".video-overlay");
        fs::create_dir_all(&work).map_err(|e| format!("video step overlay: {e}"))?;
        let _cleanup = WorkDirectory(work.clone());
        let font_option = if let Some(path) = caption_font() {
            fs::copy(path, work.join("caption-font.ttf"))
                .map_err(|e| format!("video step overlay font: {e}"))?;
            ":fontfile=caption-font.ttf"
        } else {
            ""
        };
        let scale = duration / self.duration.unwrap_or(duration).max(0.001);
        let mut filter =
            format!("[0:v]pad=ceil(iw/2)*2:ceil((ih+{header})/2)*2:0:{header}:color=0x111827");
        let mut timeline = Vec::new();
        for (index, (cue, label)) in self.cues.iter().zip(&labels).enumerate() {
            let start = if index == 0 {
                0.
            } else {
                (cue.at * scale).min(duration)
            };
            let end = self
                .cues
                .get(index + 1)
                .map_or(duration, |next| (next.at * scale).min(duration));
            if end <= start {
                continue;
            }
            let filename = format!("step-{index:04}.txt");
            fs::write(work.join(&filename), label)
                .map_err(|e| format!("video step overlay: {e}"))?;
            // textfile + expansion=none keeps %, braces, quotes and backslashes literal.
            // Paths in this filter are controlled relative names, never user input.
            filter.push_str(&format!(",drawtext=textfile={filename}:expansion=none{font_option}:fontcolor=white:fontsize={font}:x=12:y=12:line_spacing=6:enable='gte(t,{start:.6})*lt(t,{end:.6})'"));
            timeline.push(TimelineCue {
                id: &cue.id,
                action: &cue.action,
                runner_seconds: cue.at,
                video_seconds: start,
            });
        }
        filter.push_str("[captioned]");
        fs::write(work.join("filter.txt"), filter)
            .map_err(|e| format!("video step overlay: {e}"))?;
        let original = directory.join("video-original.mp4");
        fs::copy(&input, &original).map_err(|e| format!("video step overlay: {e}"))?;
        // Resolve paths before changing cwd. No input values are passed on the command line.
        let original = fs::canonicalize(original).map_err(|e| e.to_string())?;
        let rendered = directory.join("video-captioned.mp4");
        let rendered_absolute = fs::canonicalize(directory)
            .map_err(|e| e.to_string())?
            .join("video-captioned.mp4");
        let mut help = Command::new(&ffmpeg);
        help.args(["-hide_banner", "-h", "full"]);
        let help = output(help, 15).await?;
        let filter_file_option =
            if String::from_utf8_lossy(&help).contains("-filter_complex_script") {
                "-filter_complex_script"
            } else {
                "-/filter_complex"
            };
        let mut command = Command::new(ffmpeg);
        command
            .current_dir(&work)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(original)
            .args([
                filter_file_option,
                "filter.txt",
                "-map",
                "[captioned]",
                "-map",
                "0:a?",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "copy",
                "-movflags",
                "+faststart",
            ])
            .arg(&rendered_absolute);
        if let Err(error) = output(command, 120).await {
            let _ = fs::remove_file(&rendered);
            return Err(format!(
                "video step overlay failed: {error}. Install FFmpeg with drawtext (macOS: brew install ffmpeg-full), or set DEVICE_BOT_FFMPEG_PATH. Original recording retained."
            ));
        }
        fs::rename(&rendered, &input).map_err(|e| format!("video step overlay: {e}"))?;
        fs::write(
            directory.join("video-steps.json"),
            serde_json::to_vec_pretty(&timeline).unwrap(),
        )
        .map_err(|e| format!("video step overlay: {e}"))?;
        Ok(())
    }
}

#[derive(Serialize)]
struct TimelineCue<'a> {
    id: &'a str,
    action: &'a str,
    runner_seconds: f64,
    video_seconds: f64,
}

struct WorkDirectory(PathBuf);
impl Drop for WorkDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn caption_font() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEVICE_BOT_VIDEO_FONT") {
        return Some(path.into());
    }
    let mut candidates = vec![
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ];
    if let Some(windows) = std::env::var_os("WINDIR") {
        candidates.push(PathBuf::from(&windows).join("Fonts/meiryo.ttc"));
        candidates.push(PathBuf::from(windows).join("Fonts/arial.ttf"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn ffmpeg_path() -> PathBuf {
    if let Some(path) = std::env::var_os("DEVICE_BOT_FFMPEG_PATH") {
        return path.into();
    }
    for path in [
        "/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg",
        "/usr/local/opt/ffmpeg-full/bin/ffmpeg",
        "/opt/homebrew/opt/ffmpeg/bin/ffmpeg",
        "/usr/local/opt/ffmpeg/bin/ffmpeg",
    ] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    "ffmpeg".into()
}

async fn output(mut command: Command, seconds: u64) -> Result<Vec<u8>, String> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    let result = timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| "media command timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        // ffmpeg may quote caption files on errors. Do not persist its stderr/input.
        return Err(format!("media command exited with {}", result.status));
    }
    Ok(result.stdout)
}

fn wrap(text: &str, columns: usize) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let mut count = 0;
        for c in line.chars().filter(|c| !c.is_control() || *c == '\t') {
            if count == columns {
                result.push('\n');
                count = 0;
            }
            result.push(if c == '\t' { ' ' } else { c });
            count += 1;
        }
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn captions_include_resolved_test_input_without_filter_interpolation() {
        let mut overlay = VideoOverlay::new();
        overlay.mark(
            "case.password",
            "fill_field",
            &json!({"target":"password", "text":"demo%{text}\\日本語'", "sensitive":true}),
        );
        assert!(
            overlay.cues[0]
                .label
                .contains("input [password]: demo%{text}\\日本語'")
        );
        assert!(overlay.cues[0].label.contains("target: password"));
        assert!(
            wrap(&overlay.cues[0].label, 12)
                .lines()
                .all(|line| line.chars().count() <= 12)
        );
    }

    #[test]
    fn tool_input_and_scroll_direction_have_readable_captions() {
        let mut overlay = VideoOverlay::new();
        overlay.mark(
            "type",
            "call_tool",
            &json!({"arguments":{"text":"test-user"}}),
        );
        overlay.mark(
            "scroll",
            "scroll",
            &json!({"target":"list","direction":"down"}),
        );
        assert!(overlay.cues[0].label.contains("input [type]: test-user"));
        assert!(overlay.cues[1].label.contains("direction: down"));
        overlay.finish();
        assert!(overlay.duration.unwrap() > 0.);
    }

    #[tokio::test]
    async fn rendered_video_contains_changing_step_text_and_retains_original() {
        let ffmpeg = ffmpeg_path();
        let mut check = Command::new(&ffmpeg);
        check.args(["-hide_banner", "-filters"]);
        let Ok(filters) = output(check, 10).await else {
            eprintln!("Skipping video rendering test: FFmpeg not installed");
            return;
        };
        if !String::from_utf8_lossy(&filters).contains("drawtext") {
            eprintln!("Skipping video rendering test: FFmpeg drawtext unavailable");
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "robot-video-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        let _cleanup = WorkDirectory(path.clone());
        let mut create = Command::new(&ffmpeg);
        create
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=320x180:r=10:d=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path.join("video.mp4"));
        output(create, 20).await.unwrap();
        let mut overlay = VideoOverlay::new();
        overlay.mark(
            "test.input",
            "fill_field",
            &serde_json::json!({"target":"password", "text":"DEMO%{literal}'日本語\\{}"}),
        );
        overlay.cues[0].at = 0.;
        overlay.mark(
            "test.scroll",
            "scroll",
            &serde_json::json!({"direction":"down"}),
        );
        overlay.cues[1].at = 1.;
        overlay.duration = Some(2.);
        overlay.render(&path).await.unwrap();
        assert!(path.join("video-original.mp4").is_file());
        assert!(!path.join(".video-overlay").exists());
        let timeline = fs::read_to_string(path.join("video-steps.json")).unwrap();
        assert!(timeline.contains("test.input"));
        assert!(!timeline.contains("DEMO"));
        let mut frames = Vec::new();
        for time in ["0.5", "1.5"] {
            let mut extract = Command::new(&ffmpeg);
            extract
                .args(["-hide_banner", "-loglevel", "error", "-ss", time, "-i"])
                .arg(path.join("video.mp4"))
                .args([
                    "-frames:v",
                    "1",
                    "-vf",
                    "crop=320:60:0:0",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgb24",
                    "pipe:1",
                ]);
            let frame = output(extract, 15).await.unwrap();
            assert_eq!(frame.len(), 320 * 60 * 3);
            assert!(
                frame
                    .chunks_exact(3)
                    .any(|pixel| pixel.iter().all(|v| *v > 200)),
                "caption has no white text pixels"
            );
            frames.push(frame);
        }
        assert_ne!(
            frames[0], frames[1],
            "step caption did not change over time"
        );
    }
}

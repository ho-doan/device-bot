//! Step captions rendered into the recorded artifact, independently of the app UI.
use crate::runner::StepReport;
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

fn media_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

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

    pub(crate) async fn render(
        &self,
        directory: &Path,
        steps: &[StepReport],
        case_status: &str,
    ) -> Result<(), String> {
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
        let mut command = media_command(probe);
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
        let height = metadata["streams"][0]["height"]
            .as_u64()
            .filter(|h| *h > 0 && *h <= 16384)
            .ok_or("video step overlay: missing video height")?;
        let font = (width / 64).clamp(16, 30);
        let columns = (width / font).max(20) as usize;
        let labels: Vec<_> = self
            .cues
            .iter()
            .map(|cue| {
                let state = cue_state(cue, steps);
                let (heading, details) = cue.label.split_once('\n').unwrap_or((&cue.label, ""));
                let note = steps
                    .iter()
                    .find(|step| step.id == cue.id && step.action == cue.action)
                    .and_then(|step| step.note.as_deref());
                let label = match note {
                    Some(note) => format!("[{}] {heading}\n{note}\n{details}", state.label()),
                    None => format!("[{}] {}", state.label(), cue.label),
                };
                wrap(&label, columns)
            })
            .collect();
        let rows = labels
            .iter()
            .map(|label| label.lines().count())
            .max()
            .unwrap_or(1);
        // Fonts with CJK coverage have larger line metrics than their pixel size.
        // Reserve enough height so the last input line stays on the dark header.
        let header = (rows as u64 * (font * 3 / 2 + 8) + 24).div_ceil(2) * 2;
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
            let state = cue_state(cue, steps);
            filter.push_str(&format!(",drawtext=textfile={filename}:expansion=none{font_option}:fontcolor={}:fontsize={font}:x=12:y=12:line_spacing=6:enable='gte(t,{start:.6})*lt(t,{end:.6})'", state.color()));
            timeline.push(TimelineCue {
                id: &cue.id,
                action: &cue.action,
                runner_seconds: cue.at,
                video_seconds: start,
                status: state.label(),
            });
        }
        let summary = Summary::new(steps, case_status, width, height + header, font, duration);
        summary.append_filters(&mut filter, &work, font_option, font)?;
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
        let mut help = media_command(&ffmpeg);
        help.args(["-hide_banner", "-h", "full"]);
        let help = output(help, 15).await?;
        let filter_file_option =
            if String::from_utf8_lossy(&help).contains("-filter_complex_script") {
                "-filter_complex_script"
            } else {
                "-/filter_complex"
            };
        let mut command = media_command(ffmpeg);
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
        fs::write(
            directory.join("video-summary.json"),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )
        .map_err(|e| format!("video summary: {e}"))?;
        Ok(())
    }
}

#[derive(Serialize)]
struct TimelineCue<'a> {
    id: &'a str,
    action: &'a str,
    runner_seconds: f64,
    video_seconds: f64,
    status: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StepState {
    Pass,
    Fail,
    Incomplete,
}

impl StepState {
    fn from_status(status: &str) -> Self {
        match status {
            "passed" | "PASS" => Self::Pass,
            "failed" | "FAIL" => Self::Fail,
            _ => Self::Incomplete,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Incomplete => "INCOMPLETE",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Self::Pass => "0x4ade80",
            Self::Fail => "0xf87171",
            Self::Incomplete => "0xfacc15",
        }
    }
}

fn cue_state(cue: &Cue, steps: &[StepReport]) -> StepState {
    steps
        .iter()
        .find(|step| step.id == cue.id && step.action == cue.action)
        .map_or(StepState::Incomplete, |step| {
            StepState::from_status(&step.status)
        })
}

#[derive(Serialize)]
struct SummaryRow {
    id: String,
    action: String,
    status: StepState,
    note: Option<String>,
    text: String,
    y: u64,
}

#[derive(Serialize)]
struct SummaryPage {
    start_seconds: f64,
    end_seconds: f64,
    rows: Vec<SummaryRow>,
}

#[derive(Serialize)]
struct Summary {
    case_status: String,
    passed: usize,
    failed: usize,
    incomplete: usize,
    content_duration_seconds: f64,
    end_seconds: f64,
    title: String,
    footer_y: u64,
    pages: Vec<SummaryPage>,
}

impl Summary {
    fn new(
        steps: &[StepReport],
        case_status: &str,
        width: u64,
        height: u64,
        font: u64,
        start: f64,
    ) -> Self {
        let passed = steps
            .iter()
            .filter(|step| StepState::from_status(&step.status) == StepState::Pass)
            .count();
        let failed = steps
            .iter()
            .filter(|step| StepState::from_status(&step.status) == StepState::Fail)
            .count();
        let incomplete = steps.len() - passed - failed;
        let columns = (width.saturating_sub(24) / font).max(10) as usize;
        let line_height = font * 3 / 2 + 8;
        let title = wrap(
            &format!(
                "SUMMARY | RESULT: {case_status}\nPASS {passed} / FAIL {failed} / INCOMPLETE {incomplete}"
            ),
            columns,
        );
        let top = 12 + title.lines().count() as u64 * line_height;
        let footer_y = height.saturating_sub(line_height + 12);
        let capacity = footer_y.saturating_sub(top + 12).max(line_height);
        let max_lines = (capacity / line_height).max(1) as usize;
        let mut pages = vec![SummaryPage {
            start_seconds: start,
            end_seconds: start,
            rows: Vec::new(),
        }];
        let mut used = 0;
        for step in steps {
            let status = StepState::from_status(&step.status);
            let mut text = format!("[{}] {} | {}", status.label(), step.id, step.action);
            if let Some(note) = &step.note {
                text.push_str(&format!("\n{note}"));
            }
            let text = wrap(&text, columns);
            let lines: Vec<_> = text.lines().collect();
            for chunk in lines.chunks(max_lines) {
                let row_height = chunk.len() as u64 * line_height;
                let current = pages.last().unwrap();
                if !current.rows.is_empty()
                    && (used + row_height > capacity || current.rows.len() >= 12)
                {
                    pages.push(SummaryPage {
                        start_seconds: start,
                        end_seconds: start,
                        rows: Vec::new(),
                    });
                    used = 0;
                }
                pages.last_mut().unwrap().rows.push(SummaryRow {
                    id: step.id.clone(),
                    action: step.action.clone(),
                    status,
                    note: step.note.clone(),
                    text: chunk.join("\n"),
                    y: top + used,
                });
                used += row_height;
            }
        }
        let mut end = start;
        for page in &mut pages {
            let lines: usize = page.rows.iter().map(|row| row.text.lines().count()).sum();
            page.start_seconds = end;
            end += (lines as f64 * 0.7).clamp(6., 12.);
            page.end_seconds = end;
        }
        Self {
            case_status: case_status.into(),
            passed,
            failed,
            incomplete,
            content_duration_seconds: start,
            end_seconds: end,
            title,
            footer_y,
            pages,
        }
    }

    fn append_filters(
        &self,
        filter: &mut String,
        work: &Path,
        font_option: &str,
        font: u64,
    ) -> Result<(), String> {
        let start = self.content_duration_seconds;
        let extra = self.end_seconds - start;
        filter.push_str(&format!(",tpad=stop_mode=clone:stop_duration={extra:.6},drawbox=x=0:y=0:w=iw:h=ih:color=0x111827:t=fill:enable='gte(t,{start:.6})'"));
        let mut text = |filename: String,
                        value: &str,
                        color: &str,
                        y: u64,
                        from: f64,
                        to: f64|
         -> Result<(), String> {
            fs::write(work.join(&filename), value).map_err(|e| format!("video summary: {e}"))?;
            filter.push_str(&format!(",drawtext=textfile={filename}:expansion=none{font_option}:fontcolor={color}:fontsize={font}:x=12:y={y}:line_spacing=6:enable='gte(t,{from:.6})*lt(t,{to:.6})'"));
            Ok(())
        };
        text(
            "summary-title.txt".into(),
            &self.title,
            StepState::from_status(&self.case_status).color(),
            12,
            start,
            self.end_seconds,
        )?;
        for (index, page) in self.pages.iter().enumerate() {
            for (row_index, row) in page.rows.iter().enumerate() {
                text(
                    format!("summary-{index:03}-{row_index:03}.txt"),
                    &row.text,
                    row.status.color(),
                    row.y,
                    page.start_seconds,
                    page.end_seconds,
                )?;
            }
            text(
                format!("summary-page-{index:03}.txt"),
                &format!("PAGE {} / {}", index + 1, self.pages.len()),
                "white",
                self.footer_y,
                page.start_seconds,
                page.end_seconds,
            )?;
        }
        Ok(())
    }
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
    fn summary_paginates_all_step_results_and_intent_notes() {
        let steps: Vec<_> = (0..35)
            .map(|i| StepReport {
                id: format!("step-{i}"),
                action: "tap_element".into(),
                status: if i == 34 { "failed" } else { "passed" }.into(),
                note: Some("Bấm nút để mở màn hình cần kiểm tra".into()),
                error: None,
            })
            .collect();
        let summary = Summary::new(&steps, "FAIL", 720, 600, 20, 10.);
        assert_eq!(summary.passed, 34);
        assert_eq!(summary.failed, 1);
        assert!(summary.pages.len() > 1);
        assert_eq!(
            summary
                .pages
                .iter()
                .map(|page| page.rows.len())
                .sum::<usize>(),
            35
        );
        assert!(
            summary
                .pages
                .iter()
                .all(|page| page.end_seconds > page.start_seconds && page.rows.len() <= 12)
        );
        for (i, row) in summary.pages.iter().flat_map(|page| &page.rows).enumerate() {
            assert_eq!(row.id, format!("step-{i}"));
            assert!(row.text.contains("Bấm nút"));
            assert!(row.y < summary.footer_y);
        }
    }

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
        let steps = vec![
            StepReport {
                id: "test.input".into(),
                action: "fill_field".into(),
                status: "passed".into(),
                note: Some("Nhập mật khẩu kiểm thử".into()),
                error: None,
            },
            StepReport {
                id: "test.scroll".into(),
                action: "scroll".into(),
                status: "failed".into(),
                note: Some("Cuộn để tìm nội dung cần kiểm tra".into()),
                error: Some("element missing".into()),
            },
        ];
        overlay.render(&path, &steps, "FAIL").await.unwrap();
        assert!(path.join("video-original.mp4").is_file());
        assert!(!path.join(".video-overlay").exists());
        let timeline = fs::read_to_string(path.join("video-steps.json")).unwrap();
        assert!(timeline.contains("test.input"));
        assert!(!timeline.contains("DEMO"));
        let summary: Value =
            serde_json::from_slice(&fs::read(path.join("video-summary.json")).unwrap()).unwrap();
        assert_eq!(summary["passed"], 1);
        assert_eq!(summary["failed"], 1);
        assert!(summary["end_seconds"].as_f64().unwrap() > 2.);
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
            let green = time == "0.5";
            assert!(
                frame.chunks_exact(3).any(|p| if green {
                    p[1] > 160 && p[0] < 150 && p[2] < 180
                } else {
                    p[0] > 180 && p[1] < 170 && p[2] < 170
                }),
                "step caption has wrong status color"
            );
            frames.push(frame);
        }
        assert_ne!(
            frames[0], frames[1],
            "step caption did not change over time"
        );
        let summary_time = summary["pages"][0]["start_seconds"].as_f64().unwrap() + 1.;
        let mut extract = Command::new(&ffmpeg);
        extract
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &summary_time.to_string(),
                "-i",
            ])
            .arg(path.join("video.mp4"))
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ]);
        let frame = output(extract, 15).await.unwrap();
        assert!(
            frame
                .chunks_exact(3)
                .any(|p| p[1] > 160 && p[0] < 150 && p[2] < 180),
            "summary lacks green PASS row"
        );
        assert!(
            frame
                .chunks_exact(3)
                .any(|p| p[0] > 180 && p[1] < 170 && p[2] < 170),
            "summary lacks red FAIL row"
        );
    }
}

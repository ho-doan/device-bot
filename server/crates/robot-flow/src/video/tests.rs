use super::*;

#[test]
fn video_render_deadline_grows_with_duration_and_remains_bounded() {
    assert_eq!(render_timeout_seconds(0.0), 120);
    assert_eq!(render_timeout_seconds(109.0), 556);
    assert_eq!(render_timeout_seconds(10_000.0), 1800);
}
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
            element_probe: None,
            network_fault: None,
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
            element_probe: None,
            network_fault: None,
        },
        StepReport {
            id: "test.scroll".into(),
            action: "scroll".into(),
            status: "failed".into(),
            note: Some("Cuộn để tìm nội dung cần kiểm tra".into()),
            error: Some("element missing".into()),
            element_probe: None,
            network_fault: None,
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

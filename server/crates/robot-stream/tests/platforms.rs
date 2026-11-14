//! Opt-in real capture -> WebRTC -> H.264 decode and input checks.
//! Use only disposable fixtures. No physical test is silently counted as PASS.
use robot_core::{ElementType, Point, Robot};
use robot_stream::{StreamAnswer, StreamManager};
use std::{path::PathBuf, time::Duration};
use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp::{codecs::h264::H264Packet, packetizer::Depacketizer},
    rtp_transceiver::{
        RTCRtpTransceiverInit,
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
};

struct HelperCleanup(PathBuf);
impl Drop for HelperCleanup {
    fn drop(&mut self) {
        // Only PIDs written by this test's xcodebuild wrapper are owned here.
        if let Ok(pids) = std::fs::read_to_string(&self.0) {
            for pid in pids.lines().filter_map(|pid| pid.parse::<u32>().ok()) {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .output();
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires explicit DEVICE_BOT_LIVE_PLATFORM/DEVICE and a disposable fixture"]
async fn real_platform_live_decodes_controls_reopens_and_stops() {
    let platform = std::env::var("DEVICE_BOT_LIVE_PLATFORM").expect("explicit platform");
    let mut device = std::env::var("DEVICE_BOT_LIVE_DEVICE").expect("explicit device");
    let artifacts =
        PathBuf::from(std::env::var("DEVICE_BOT_LIVE_ARTIFACTS").expect("artifact directory"));
    tokio::fs::create_dir_all(&artifacts).await.unwrap();
    let pid_file = artifacts.join("xctest-pids");
    let _helper_cleanup = HelperCleanup(pid_file.clone());
    let xcodebuild = if matches!(platform.as_str(), "simulator" | "ios-real") {
        // Prevent an abandoned XCTest run from restarting the helper underneath
        // the next attempt. Retain the native driver's actual build/agent path.
        tokio::fs::write(&pid_file, "").await.unwrap();
        let wrapper = artifacts.join("xcodebuild-wrapper.py");
        tokio::fs::write(&wrapper, "#!/usr/bin/env python3\nimport os,sys\nif 'test-without-building' in sys.argv:\n with open(os.path.join(os.path.dirname(__file__),'xctest-pids'),'a') as f: f.write(str(os.getpid())+'\\n')\nos.execvp('xcodebuild',['xcodebuild',*sys.argv[1:]])\n").await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        wrapper.to_string_lossy().into_owned()
    } else {
        "xcodebuild".into()
    };
    let agent_token = (platform == "ios-real").then(|| uuid::Uuid::new_v4().to_string());
    let simulator_agent = robot_ios_real::XCTestAgentBridge::new(agent_token.clone());
    let bind = if platform == "ios-real" {
        "0.0.0.0:0"
    } else {
        "127.0.0.1:0"
    };
    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    let agent_address = listener.local_addr().unwrap();
    let router = simulator_agent.router();
    let agent_server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let robot: Box<dyn Robot> = match platform.as_str() {
        "macos" => Box::new(robot_macos::MacosRobot::new(&artifacts)),
        "simulator" => Box::new(
            robot_ios_sim::IosSimulatorRobot::with_xctest_runner_and_agent(
                "xcrun",
                xcodebuild,
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../robot-ios-sim/xctest-runner/MobileMcpSimulatorRunner.xcodeproj"),
                &artifacts,
                simulator_agent,
                "127.0.0.1",
                agent_address.port(),
            ),
        ),
        "ios-real" => {
            let runner =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../robot-ios-real/xctest-runner");
            let launcher = robot_ios_real::XCTestAgentLauncher::new(
                xcodebuild,
                runner.join("MobileMcpRealRunner.xcodeproj"),
                runner.join("build"),
                std::env::var("DEVICE_BOT_LIVE_IOS_HOSTS")
                    .expect("explicit Mac LAN address reachable from the iPhone"),
                agent_address.port(),
                agent_token,
            )
            .unwrap();
            Box::new(
                robot_ios_real::IosXCTestRobot::with_artifact_dir(
                    "xcrun",
                    simulator_agent,
                    &artifacts,
                )
                .with_agent_launcher(launcher),
            )
        }
        "android" => Box::new(robot_android::AndroidRobot::new("adb", &artifacts)),
        "web" => Box::new(robot_chrome::ChromeRobot::new(
            std::env::var("DEVICE_BOT_LIVE_CHROME_ENDPOINT").expect("disposable Chrome endpoint"),
        )),
        _ => panic!("unsupported test platform {platform}"),
    };
    if platform == "web" {
        robot.boot_device(&device).await.unwrap();
        device = robot
            .list_devices()
            .await
            .unwrap()
            .into_iter()
            .find(|device| device.id != "chrome:browser")
            .expect("Chrome fixture tab")
            .id;
        robot
            .open_url(
                &device,
                &std::env::var("DEVICE_BOT_LIVE_URL").expect("fixture URL"),
                None,
            )
            .await
            .unwrap();
    } else if let Ok(app) = std::env::var("DEVICE_BOT_LIVE_APP") {
        robot.launch_app(&device, &app).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let streams = StreamManager::default();
    for round in 1..=3 {
        let mut media = MediaEngine::default();
        media
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/H264".into(),
                        clock_rate: 90000,
                        sdp_fmtp_line:
                            "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                                .into(),
                        ..Default::default()
                    },
                    payload_type: 102,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .unwrap();
        let viewer = APIBuilder::new()
            .with_media_engine(media)
            .build()
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        viewer
            .add_transceiver_from_kind(
                RTPCodecType::Video,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();
        let (tx, mut received) = tokio::sync::mpsc::channel(1);
        viewer.on_track(Box::new(move |track, _, _| {
            let tx = tx.clone();
            Box::pin(async move {
                assert_eq!(
                    track.codec().capability.mime_type.to_lowercase(),
                    "video/h264"
                );
                let mut decoder = H264Packet::default();
                let mut annex_b = Vec::new();
                let mut frames = 0;
                while let Ok((packet, _)) = track.read_rtp().await {
                    annex_b.extend_from_slice(&decoder.depacketize(&packet.payload).unwrap());
                    if packet.header.marker {
                        frames += 1;
                    }
                    // A static native window need not emit five frames. One
                    // complete decodable frame is the connection contract.
                    if frames >= 1 {
                        let _ = tx.send(annex_b).await;
                        break;
                    }
                }
            })
        }));
        let offer = viewer.create_offer(None).await.unwrap();
        let mut gathered = viewer.gathering_complete_promise().await;
        viewer.set_local_description(offer).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), gathered.recv())
            .await
            .unwrap();
        let started = std::time::Instant::now();
        let offer = viewer.local_description().await.unwrap().sdp;
        let mut native = None;
        let answer = if std::env::var("DEVICE_BOT_LIVE_WEB_MODE").as_deref() == Ok("native") {
            assert_eq!(platform, "web");
            let session = tokio::time::timeout(
                Duration::from_secs(60),
                robot.negotiate_native_video(&device, &offer),
            )
            .await
            .expect("bounded native capture")
            .unwrap()
            .expect("native capture required; fallback cannot pass this test");
            let answer = StreamAnswer {
                stream_id: format!("native-test-{round}"),
                sdp: session.sdp.clone(),
                width: session.width,
                height: session.height,
                context: robot.stream_context(&device).await.unwrap(),
            };
            native = Some(session);
            answer
        } else {
            tokio::time::timeout(
                Duration::from_secs(300),
                streams.start(robot.as_ref(), &device, offer),
            )
            .await
            .expect("bounded stream startup")
            .unwrap()
        };
        assert!(answer.width > 0 && answer.height > 0);
        println!(
            "{platform} round {round}: negotiated {}x{} in {:?}; context={}",
            answer.width,
            answer.height,
            started.elapsed(),
            answer.context
        );
        viewer
            .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
            .await
            .unwrap();

        let video = tokio::time::timeout(Duration::from_secs(25), received.recv())
            .await
            .expect("first complete video frame")
            .expect("H.264 track");
        let input_path = artifacts.join(format!("round-{round}.h264"));
        let image_path = artifacts.join(format!("round-{round}.png"));
        tokio::fs::write(&input_path, &video).await.unwrap();
        let decoded = tokio::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "h264",
                "-i",
            ])
            .arg(&input_path)
            .args(["-frames:v", "1"])
            .arg(&image_path)
            .output()
            .await
            .unwrap();
        assert!(
            decoded.status.success(),
            "H.264 decode: {}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert!(tokio::fs::metadata(&image_path).await.unwrap().len() > 100);

        let elements = robot.list_elements(&device).await.unwrap();
        tokio::fs::write(
            artifacts.join(format!("round-{round}-before.json")),
            serde_json::to_vec_pretty(&elements).unwrap(),
        )
        .await
        .unwrap();
        let field = elements
            .iter()
            .find(|element| {
                element.enabled
                    && (element.element_type == Some(ElementType::TextField)
                        || element
                            .class_name
                            .to_ascii_lowercase()
                            .contains("textfield")
                        || element.class_name.contains("EditText"))
            })
            .expect("fixture editor");
        robot
            .tap(
                &device,
                Point {
                    x: (field.bounds.left + field.bounds.right) as f64 / 2.,
                    y: (field.bounds.top + field.bounds.bottom) as f64 / 2.,
                },
            )
            .await
            .unwrap();
        // Native iOS clear is unsupported; append a unique numeric probe instead.
        if !matches!(platform.as_str(), "simulator" | "ios-real") {
            robot.clear_text(&device).await.unwrap();
        }
        let probe = format!("314159{round}");
        robot.type_text(&device, &probe).await.unwrap();
        let after = robot.list_elements(&device).await.unwrap();
        tokio::fs::write(
            artifacts.join(format!("round-{round}-after.json")),
            serde_json::to_vec_pretty(&after).unwrap(),
        )
        .await
        .unwrap();
        assert!(
            after.iter().any(|element| element
                .value
                .as_deref()
                .is_some_and(|v| v.contains(&probe))
                || element.text.as_deref().is_some_and(|v| v.contains(&probe))),
            "input did not reach fixture editor: {after:?}"
        );
        if let Some(native) = native {
            native.stop.send(true).unwrap();
            tokio::time::timeout(Duration::from_secs(15), native.done)
                .await
                .expect("bounded native capture stop")
                .unwrap();
        } else {
            tokio::time::timeout(Duration::from_secs(15), streams.stop(&answer.stream_id))
                .await
                .expect("bounded stream stop")
                .unwrap();
            streams.stop(&answer.stream_id).await.unwrap();
        }
        viewer.close().await.unwrap();
        println!(
            "{platform} round {round}: decoded {} bytes, verified input {probe}, stopped",
            video.len()
        );
    }
    streams.stop_all().await;
    robot.close_session().await.unwrap();
    agent_server.abort();
}

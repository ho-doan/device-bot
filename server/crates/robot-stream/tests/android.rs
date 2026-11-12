//! End-to-end device encoder -> ADB -> gateway -> ICE/DTLS/SRTP -> receiver.
use robot_stream::StreamManager;
use std::{sync::Arc, time::Duration};
use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::{
        RTCRtpTransceiverInit, rtp_codec::RTPCodecType,
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
};

#[tokio::test]
#[ignore = "requires DEVICE_BOT_TEST_ANDROID, ADB and scrcpy"]
async fn physical_device_stream_arrives_over_webrtc_and_stops() {
    let id = std::env::var("DEVICE_BOT_TEST_ANDROID").expect("DEVICE_BOT_TEST_ANDROID");
    let forwards_before = tokio::process::Command::new("adb")
        .args(["forward", "--list"])
        .output()
        .await
        .unwrap()
        .stdout;
    let robot = robot_android::AndroidRobot::new("adb", std::env::temp_dir());
    let mut media = MediaEngine::default();
    media.register_default_codecs().unwrap();
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    viewer.on_track(Box::new(move |track, _, _| {
        let tx = tx.clone();
        Box::pin(async move {
            assert_eq!(
                track.codec().capability.mime_type.to_lowercase(),
                "video/h264"
            );
            let mut packets = 0;
            while let Ok((packet, _)) = track.read_rtp().await {
                if !packet.payload.is_empty() {
                    packets += 1;
                }
                if packets >= 5 {
                    let _ = tx.send(packets).await;
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
    let streams = Arc::new(StreamManager::default());
    let answer = streams
        .start(&robot, &id, viewer.local_description().await.unwrap().sdp)
        .await
        .unwrap();
    assert!(answer.width > 0 && answer.height > 0);
    viewer
        .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
        .await
        .unwrap();
    let packets = tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .unwrap()
        .unwrap();
    println!(
        "Received {packets} H.264 RTP packets from {}x{} physical Android",
        answer.width, answer.height
    );
    streams.stop(&answer.stream_id).await.unwrap();
    streams.stop(&answer.stream_id).await.unwrap();
    viewer.close().await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let forwards_after = tokio::process::Command::new("adb")
        .args(["forward", "--list"])
        .output()
        .await
        .unwrap()
        .stdout;
    assert_eq!(
        forwards_before, forwards_after,
        "capture leaked an ADB forward"
    );
}

#[tokio::test]
async fn malformed_offer_never_starts_capture_and_stop_is_idempotent() {
    let robot = robot_android::AndroidRobot::new("must-not-run-adb", std::env::temp_dir());
    let streams = StreamManager::default();
    assert!(
        streams
            .start(&robot, "phone", "not an SDP offer".into())
            .await
            .is_err()
    );
    streams.stop("unknown").await.unwrap();
    streams.stop("unknown").await.unwrap();
    streams.stop_all().await;
}

#[tokio::test]
#[ignore = "requires physical Android and installed com.example.android_app login fixture"]
async fn clears_the_focused_sample_editor_over_scrcpy_control() {
    use robot_core::{ElementType, Point, Robot};
    let id = std::env::var("DEVICE_BOT_TEST_ANDROID").expect("DEVICE_BOT_TEST_ANDROID");
    let robot = robot_android::AndroidRobot::new("adb", std::env::temp_dir());
    robot
        .launch_app(&id, "com.example.android_app")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut elements = robot.list_elements(&id).await.unwrap();
    // The fixture may already be in its demo workspace. Open its login screen.
    if !elements
        .iter()
        .any(|element| element.element_type == Some(ElementType::TextField))
    {
        for resource in ["account-avatar-control", "sign-out-button"] {
            let target = elements
                .iter()
                .find(|element| element.resource_id.as_deref() == Some(resource))
                .expect("sample fixture navigation control");
            robot
                .tap(
                    &id,
                    Point {
                        x: (target.bounds.left + target.bounds.right) as f64 / 2.,
                        y: (target.bounds.top + target.bounds.bottom) as f64 / 2.,
                    },
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            elements = robot.list_elements(&id).await.unwrap();
        }
    }
    let editor = elements
        .iter()
        .find(|element| element.element_type == Some(ElementType::TextField))
        .expect("sample login editor must be visible");
    let original = editor.text.clone().unwrap_or_default();
    robot
        .tap(
            &id,
            Point {
                x: (editor.bounds.left + editor.bounds.right) as f64 / 2.,
                y: (editor.bounds.top + editor.bounds.bottom) as f64 / 2.,
            },
        )
        .await
        .unwrap();
    let stream = robot.start_video_stream(&id).await.unwrap();
    let result = async {
        robot.clear_text(&id).await?;
        robot.type_text(&id, "device_bot_clear_probe").await?;
        let typed = robot.list_elements(&id).await?;
        if !typed.iter().any(|element| {
            element.focused == Some(true)
                && element.text.as_deref() == Some("device_bot_clear_probe")
        }) {
            return Err(robot_core::RobotError::CommandFailed(
                "sample text did not reach focused editor".into(),
            ));
        }
        robot.clear_text(&id).await?;
        let cleared = robot.list_elements(&id).await?;
        if !cleared.iter().any(|element| {
            element.focused == Some(true)
                && element.element_type == Some(ElementType::TextField)
                && element.text.as_deref().unwrap_or("").is_empty()
        }) {
            return Err(robot_core::RobotError::CommandFailed(
                "focused sample editor was not cleared".into(),
            ));
        }
        Ok::<(), robot_core::RobotError>(())
    }
    .await;
    let _ = robot.clear_text(&id).await;
    if !original.is_empty() {
        let _ = robot.type_text(&id, &original).await;
    }
    drop(stream);
    tokio::time::sleep(Duration::from_millis(500)).await;
    result.unwrap();
    println!("Focused sample editor: type -> clear -> empty verified on physical Android");
}

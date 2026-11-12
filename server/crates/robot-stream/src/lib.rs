//! Shared media gateway. MCP carries SDP and lifecycle commands, never video.
use robot_core::{Robot, RobotError};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, watch};
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_H264, MediaEngine},
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

fn failure(error: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("WebRTC: {error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamAnswer {
    pub stream_id: String,
    pub sdp: String,
    pub width: u32,
    pub height: u32,
}
struct Session {
    stop: watch::Sender<bool>,
    done: tokio::task::JoinHandle<()>,
}
#[derive(Default)]
pub struct StreamManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}
impl StreamManager {
    pub async fn start(
        &self,
        robot: &dyn Robot,
        device_id: &str,
        offer: String,
    ) -> Result<StreamAnswer, RobotError> {
        if offer.len() > 128 * 1024 {
            return Err(RobotError::InvalidInput("SDP offer is too large".into()));
        }
        let offer = RTCSessionDescription::offer(offer)
            .map_err(|e| RobotError::InvalidInput(e.to_string()))?;
        let mut media = MediaEngine::default();
        let capability = RTCRtpCodecCapability {
            mime_type: MIME_TYPE_H264.into(),
            clock_rate: 90000,
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                .into(),
            ..Default::default()
        };
        media
            .register_codec(
                RTCRtpCodecParameters {
                    capability: capability.clone(),
                    payload_type: 102,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(failure)?;
        let registry =
            register_default_interceptors(Registry::new(), &mut media).map_err(failure)?;
        let api = APIBuilder::new()
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .build();
        let peer = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .map_err(failure)?,
        );
        let (stop, mut stopped) = watch::channel(false);
        let (state_tx, mut state_rx) = watch::channel(RTCPeerConnectionState::New);
        peer.on_peer_connection_state_change(Box::new(move |state| {
            state_tx.send_replace(state);
            Box::pin(async {})
        }));
        let track = Arc::new(TrackLocalStaticSample::new(
            capability,
            "screen".into(),
            "device-bot".into(),
        ));
        // Keep negotiation cancellation bounded: the worker owns and closes the peer
        // even when the requesting HTTP/MCP future is dropped.
        let peer_worker = peer.clone();
        let (source_tx, source_rx) = tokio::sync::oneshot::channel();
        let weak_sessions = Arc::downgrade(&self.sessions);
        let id = uuid::Uuid::new_v4().to_string();
        let worker_id = id.clone();
        let worker_track = track.clone();
        let done = tokio::spawn(async move {
            let pump = async {
                let mut source: robot_core::VideoStream = source_rx.await.map_err(failure)?;
                tokio::time::timeout(Duration::from_secs(20), async {
                    loop {
                        let state = *state_rx.borrow_and_update();
                        match state {
                            RTCPeerConnectionState::Connected => break Ok(()),
                            RTCPeerConnectionState::Failed
                            | RTCPeerConnectionState::Closed
                            | RTCPeerConnectionState::Disconnected => {
                                break Err(failure("peer disconnected"));
                            }
                            _ => {
                                state_rx.changed().await.map_err(failure)?;
                            }
                        }
                    }
                })
                .await
                .map_err(failure)??;
                let mut previous = None;
                let mut started = false;
                loop {
                    tokio::select! {
                        frame = source.frames.recv() => {
                            let frame = frame.ok_or_else(|| failure("capture ended"))??;
                            if !started && !frame.keyframe { continue; }
                            started = true;
                            let duration = previous.map(|pts: u64| Duration::from_micros(frame.pts_us.saturating_sub(pts).clamp(1, 1_000_000)))
                                .unwrap_or(Duration::from_micros(33_333));
                            previous = Some(frame.pts_us);
                            worker_track.write_sample(&Sample { data: frame.data.into(), duration, ..Default::default() }).await.map_err(failure)?;
                        }
                        _ = state_rx.changed() => {
                            if matches!(*state_rx.borrow(), RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) {
                                return Err(failure("peer disconnected"));
                            }
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<(), RobotError>(())
            };
            tokio::select! {
                _ = stopped.changed() => {},
                result = pump => { if let Err(error) = result { eprintln!("{error}"); } },
            }
            let _ = peer_worker.close().await;
            if let Some(sessions) = weak_sessions.upgrade() {
                sessions.lock().await.remove(&worker_id);
            }
        });
        let negotiated = async {
            let rtp = peer
                .add_track(track as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .map_err(failure)?;
            // RTCP must be consumed for NACK/interceptor processing; close unblocks it.
            tokio::spawn(async move {
                let mut buffer = [0; 1500];
                while rtp.read(&mut buffer).await.is_ok() {}
            });
            peer.set_remote_description(offer).await.map_err(failure)?;
            let answer = peer.create_answer(None).await.map_err(failure)?;
            let mut gathered = peer.gathering_complete_promise().await;
            peer.set_local_description(answer).await.map_err(failure)?;
            tokio::time::timeout(Duration::from_secs(10), gathered.recv())
                .await
                .map_err(failure)?;
            let sdp = peer
                .local_description()
                .await
                .ok_or_else(|| failure("missing SDP answer"))?
                .sdp;
            let source =
                tokio::time::timeout(Duration::from_secs(30), robot.start_video_stream(device_id))
                    .await
                    .map_err(failure)??;
            let (width, height) = (source.width, source.height);
            Ok::<_, RobotError>((
                StreamAnswer {
                    stream_id: id.clone(),
                    sdp,
                    width,
                    height,
                },
                source,
            ))
        }
        .await;
        match negotiated {
            Ok((answer, source)) => {
                self.sessions
                    .lock()
                    .await
                    .insert(id, Session { stop, done });
                source_tx
                    .send(source)
                    .map_err(|_| failure("capture worker stopped"))?;
                Ok(answer)
            }
            Err(error) => {
                drop(stop);
                let _ = done.await;
                Err(error)
            }
        }
    }
    pub async fn stop(&self, id: &str) -> Result<(), RobotError> {
        let session = self.sessions.lock().await.remove(id);
        if let Some(session) = session {
            let _ = session.stop.send(true);
            let _ = session.done.await;
        }
        Ok(()) // idempotent, including an already disconnected peer
    }
    pub async fn stop_all(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        for (_, session) in sessions {
            let _ = session.stop.send(true);
            let _ = session.done.await;
        }
    }
}

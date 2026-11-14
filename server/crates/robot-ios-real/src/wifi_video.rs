//! ReplayKit uploads complete hardware-encoded access units over the paired LAN.
//! No screenshots, public discovery endpoint, or shared long-lived upload token.
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use robot_core::{RobotError, VideoFrame, VideoStream};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, watch};

const MAX_FRAME: usize = 2 * 1024 * 1024;
const SETUP_LIFETIME: Duration = Duration::from_secs(180);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);

fn failure(message: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("iOS Wi-Fi Live: {message}"))
}

#[derive(Clone, Default)]
pub(crate) struct WifiVideoHub {
    sessions: Arc<Mutex<HashMap<String, Arc<WifiSource>>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BroadcastConfiguration {
    pub endpoint: String,
    pub token: String,
    pub expires_at: u64,
}

pub(crate) struct WifiSource {
    id: String,
    token: String,
    device_id: String,
    logical: (f64, f64),
    setup_deadline: std::sync::Mutex<tokio::time::Instant>,
    state: Mutex<UploadState>,
    tx: mpsc::Sender<Result<VideoFrame, RobotError>>,
    rx: Mutex<Option<mpsc::Receiver<Result<VideoFrame, RobotError>>>>,
    dimensions: watch::Sender<Option<(u32, u32)>>,
    stopped: watch::Sender<bool>,
    stop_reason: std::sync::Mutex<Option<String>>,
    claimed: AtomicBool,
}

struct UploadState {
    dimensions: Option<(u32, u32)>,
    previous_pts: Option<u64>,
    heartbeat: tokio::time::Instant,
}

impl WifiVideoHub {
    pub fn router(&self) -> Router {
        Router::new()
            .route(
                "/wifi-video/{id}",
                post(upload).get(control).delete(finished),
            )
            .layer(DefaultBodyLimit::max(MAX_FRAME))
            .with_state(self.clone())
    }

    pub async fn prepare(&self, device_id: &str, logical: (f64, f64)) -> (Arc<WifiSource>, bool) {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, source| !source.expired() && !*source.stopped.borrow());
        if let Some(source) = sessions
            .values()
            .find(|source| source.device_id == device_id)
        {
            return (source.clone(), false);
        }
        let (tx, rx) = mpsc::channel(8);
        let (dimensions, _) = watch::channel(None);
        let (stopped, _) = watch::channel(false);
        let now = tokio::time::Instant::now();
        let source = Arc::new(WifiSource {
            id: uuid::Uuid::new_v4().to_string(),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            device_id: device_id.into(),
            logical,
            setup_deadline: std::sync::Mutex::new(now + Duration::from_secs(360)),
            state: Mutex::new(UploadState {
                dimensions: None,
                previous_pts: None,
                heartbeat: now,
            }),
            tx,
            rx: Mutex::new(Some(rx)),
            dimensions,
            stopped,
            stop_reason: std::sync::Mutex::new(None),
            claimed: AtomicBool::new(false),
        });
        sessions.insert(source.id.clone(), source.clone());
        (source, true)
    }

    async fn authenticate(
        &self,
        id: &str,
        headers: &HeaderMap,
    ) -> Result<Arc<WifiSource>, StatusCode> {
        let source = self
            .sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or(StatusCode::NOT_FOUND)?;
        let supplied = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if supplied.len() != source.token.len()
            || supplied
                .bytes()
                .zip(source.token.bytes())
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                != 0
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if source.expired() || *source.stopped.borrow() {
            source.stop();
            return Err(StatusCode::GONE);
        }
        Ok(source)
    }
}

impl WifiSource {
    // A session that hasn't produced its first frame is a bounded consent/setup lease.
    fn expired(&self) -> bool {
        !self.claimed.load(Ordering::Relaxed)
            && tokio::time::Instant::now() > *self.setup_deadline.lock().unwrap()
    }

    pub fn configuration(&self, host: &str, port: u16) -> BroadcastConfiguration {
        *self.setup_deadline.lock().unwrap() = tokio::time::Instant::now() + SETUP_LIFETIME;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.into()
        };
        BroadcastConfiguration {
            endpoint: format!("http://{host}:{port}/wifi-video/{}", self.id),
            token: self.token.clone(),
            expires_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + SETUP_LIFETIME.as_secs(),
        }
    }

    pub fn stop(&self) {
        self.stop_with_reason("screen sharing stopped; reopen Live and start a new broadcast");
    }

    fn stop_with_reason(&self, reason: impl Into<String>) {
        let mut saved = self.stop_reason.lock().unwrap();
        if saved.is_none() {
            let reason = reason.into();
            eprintln!("iOS Wi-Fi Live: {reason}");
            *saved = Some(reason);
        }
        self.stopped.send_replace(true);
    }

    pub async fn ready(&self, timeout: Duration) -> Result<(u32, u32), RobotError> {
        let mut dimensions = self.dimensions.subscribe();
        let mut stopped = self.stopped.subscribe();
        tokio::time::timeout(timeout, async {
            loop {
                if *stopped.borrow() { return Err(failure(self.stop_reason.lock().unwrap().as_deref().unwrap_or("screen sharing stopped"))); }
                if let Some(size) = *dimensions.borrow_and_update() { return Ok(size); }
                tokio::select! {
                    result = dimensions.changed() => result.map_err(failure)?,
                    result = stopped.changed() => result.map_err(failure)?,
                }
            }
        }).await.map_err(|_| failure("On the iPhone, open Device Bot Broadcast, allow Local Network, tap the broadcast button and Start Broadcast; then retry Live. The first video frame has not arrived."))?
    }

    pub async fn start(self: &Arc<Self>) -> Result<VideoStream, RobotError> {
        let (width, height) = self.ready(Duration::from_secs(1)).await?;
        let mut rx = self.rx.lock().await.take().ok_or_else(|| {
            failure("this broadcast already has a Live viewer; close it before opening another")
        })?;
        self.claimed.store(true, Ordering::Relaxed);
        let (tx, frames) = mpsc::channel(2);
        let source = self.clone();
        tokio::spawn(async move {
            // Receiver ownership is the capture lease. Dropping it revokes uploads,
            // and the extension's next control request ends device-side capture.
            let mut stopped = source.stopped.subscribe();
            while !*stopped.borrow() {
                let result = tokio::select! {
                    _ = tx.closed() => break,
                    _ = stopped.changed() => break,
                    frame = rx.recv() => frame,
                    _ = tokio::time::sleep(HEARTBEAT_TIMEOUT) => {
                        if source.state.lock().await.heartbeat.elapsed() >= HEARTBEAT_TIMEOUT {
                            Some(Err(failure("broadcast connection lost")))
                        } else { continue; }
                    }
                };
                let Some(frame) = result else { break };
                let failed = frame.is_err();
                if tx.send(frame).await.is_err() || failed {
                    break;
                }
            }
            source.stop();
        });
        Ok(VideoStream {
            width,
            height,
            frames,
        })
    }
}

fn header<T: std::str::FromStr>(headers: &HeaderMap, name: &str) -> Result<T, StatusCode> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or(StatusCode::BAD_REQUEST)
}

// Parse every Annex-B NAL, rather than trusting client keyframe flags or searching
// arbitrary byte windows for an IDR. Parameter sets must accompany the first IDR.
fn nal_types(bytes: &[u8]) -> Result<Vec<u8>, StatusCode> {
    let mut offset = 0;
    let mut types = Vec::new();
    while offset < bytes.len() {
        let prefix = if bytes[offset..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[offset..].starts_with(&[0, 0, 1]) {
            3
        } else {
            return Err(StatusCode::BAD_REQUEST);
        };
        let start = offset + prefix;
        let nal = *bytes.get(start).ok_or(StatusCode::BAD_REQUEST)?;
        if nal & 0x80 != 0 || nal & 31 == 0 {
            return Err(StatusCode::BAD_REQUEST);
        }
        types.push(nal & 31);
        offset = (start + 1..bytes.len())
            .find(|&i| bytes[i..].starts_with(&[0, 0, 1]) || bytes[i..].starts_with(&[0, 0, 0, 1]))
            .unwrap_or(bytes.len());
    }
    if !types.iter().any(|t| matches!(t, 1 | 5)) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(types)
}

async fn upload(
    State(hub): State<WifiVideoHub>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, StatusCode> {
    let source = hub.authenticate(&id, &headers).await?;
    let width: u32 = header(&headers, "x-video-width")?;
    let height: u32 = header(&headers, "x-video-height")?;
    let pts_us: u64 = header(&headers, "x-video-pts-us")?;
    if body.is_empty()
        || body.len() > MAX_FRAME
        || !(2..=4096).contains(&width)
        || !(2..=4096).contains(&height)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let types = nal_types(&body)?;
    let keyframe = types.contains(&5);
    let mut state = source.state.lock().await;
    if crate::live::validate_geometry(source.logical.0, source.logical.1, width, height).is_err()
        || state.dimensions.is_some_and(|size| size != (width, height))
    {
        source.stop_with_reason(format!("capture geometry changed: XCTest {}x{}, ReplayKit {width}x{height}; reopen Live after rotation", source.logical.0, source.logical.1));
        return Err(StatusCode::CONFLICT);
    }
    if state.previous_pts.is_some_and(|pts| pts_us <= pts) {
        return Err(StatusCode::CONFLICT);
    }
    if state.dimensions.is_none() && !(keyframe && types.contains(&7) && types.contains(&8)) {
        return Err(StatusCode::BAD_REQUEST);
    }
    state.heartbeat = tokio::time::Instant::now();
    let frame = VideoFrame {
        data: body.to_vec(),
        pts_us,
        keyframe,
    };
    // Serial uploads preserve the encoder's reference-frame chain. Backpressure
    // is bounded, and failed uploads stop the broadcast instead of dropping deltas.
    tokio::time::timeout(Duration::from_secs(5), source.tx.send(Ok(frame)))
        .await
        .map_err(|_| {
            source.stop();
            StatusCode::REQUEST_TIMEOUT
        })?
        .map_err(|_| {
            source.stop();
            StatusCode::GONE
        })?;
    if state.dimensions.is_none() {
        eprintln!(
            "iOS Wi-Fi Live: received first H.264 keyframe, {width}x{height}, {} bytes",
            body.len()
        );
    }
    state.dimensions = Some((width, height));
    state.previous_pts = Some(pts_us);
    source.dimensions.send_replace(Some((width, height)));
    Ok(StatusCode::NO_CONTENT)
}

async fn control(
    State(hub): State<WifiVideoHub>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, HeaderMap), StatusCode> {
    let source = hub.authenticate(&id, &headers).await?;
    let mut state = source.state.lock().await;
    state.heartbeat = tokio::time::Instant::now();
    let mut response = HeaderMap::new();
    response.insert(
        "x-broadcast-state",
        if state.dimensions.is_some() {
            "sharing"
        } else {
            "ready"
        }
        .parse()
        .unwrap(),
    );
    Ok((StatusCode::NO_CONTENT, response))
}

async fn finished(
    State(hub): State<WifiVideoHub>,
    Path(id): Path<String>,
    headers: HeaderMap,
    reason: Bytes,
) -> Result<StatusCode, StatusCode> {
    let source = hub.authenticate(&id, &headers).await?;
    let reason = String::from_utf8_lossy(&reason);
    let reason: String = reason
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect();
    if reason.is_empty() {
        source.stop();
    } else {
        source.stop_with_reason(reason);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    const IDR: &[u8] = &[
        0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 1, 0, 0, 0, 1, 0x65, 1,
    ];
    fn request(
        source: &WifiSource,
        token: &str,
        width: u32,
        height: u32,
        pts: u64,
        data: &[u8],
    ) -> Request<Body> {
        Request::post(format!("/wifi-video/{}", source.id))
            .header("authorization", format!("Bearer {token}"))
            .header("x-video-width", width)
            .header("x-video-height", height)
            .header("x-video-pts-us", pts)
            .body(Body::from(data.to_vec()))
            .unwrap()
    }

    #[tokio::test]
    async fn authenticated_device_bound_frames_reach_stream_and_drop_revokes_capture() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone-a", (430., 932.)).await;
        let (other, _) = hub.prepare("phone-b", (430., 932.)).await;
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &other.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let mut stream = source.start().await.unwrap();
        let frame = stream.frames.recv().await.unwrap().unwrap();
        assert_eq!(frame.data, IDR);
        assert!(frame.keyframe);
        assert_eq!(
            (stream.width, stream.height, frame.pts_us),
            (590, 1280, 100)
        );
        assert!(source.start().await.is_err());
        drop(stream);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !*source.stopped.borrow() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 590, 1280, 200, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::GONE
        );
        let (reopened, new) = hub.prepare("phone-a", (430., 932.)).await;
        assert!(new);
        assert_ne!(source.token, reopened.token);
    }

    #[tokio::test]
    async fn rejects_corruption_missing_parameter_sets_stale_time_and_rotation() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone", (430., 932.)).await;
        for data in [
            vec![],
            vec![1, 2, 3],
            vec![0, 0, 1, 0x65, 1],
            vec![0, 0, 1, 0xe5, 1],
        ] {
            assert_eq!(
                hub.router()
                    .oneshot(request(&source, &source.token, 590, 1280, 100, &data))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 1280, 590, 200, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert!(*source.stopped.borrow());
    }

    #[tokio::test]
    async fn explicit_device_finish_closes_the_video_receiver() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone", (430., 932.)).await;
        hub.router()
            .oneshot(request(&source, &source.token, 590, 1280, 100, IDR))
            .await
            .unwrap();
        let mut stream = source.start().await.unwrap();
        stream.frames.recv().await.unwrap().unwrap();
        let req = Request::delete(format!("/wifi-video/{}", source.id))
            .header("authorization", format!("Bearer {}", source.token))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            hub.router().oneshot(req).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.frames.recv())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_oversized_upload_before_queueing_a_frame() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone", (430., 932.)).await;
        assert_eq!(
            hub.router()
                .oneshot(request(
                    &source,
                    &source.token,
                    590,
                    1280,
                    100,
                    &vec![0; MAX_FRAME + 1]
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(source.dimensions.borrow().is_none());
    }

    #[tokio::test]
    async fn expired_pairing_is_revoked_and_replaced_without_accepting_a_late_frame() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone", (430., 932.)).await;
        *source.setup_deadline.lock().unwrap() =
            tokio::time::Instant::now() - Duration::from_secs(1);
        assert_eq!(
            hub.router()
                .oneshot(request(&source, &source.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::GONE
        );
        let (next, created) = hub.prepare("phone", (430., 932.)).await;
        assert!(created);
        assert_ne!(next.id, source.id);
        assert_eq!(
            hub.router()
                .oneshot(request(&next, &source.token, 590, 1280, 100, IDR))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_stopped_preparation_reports_the_device_encoder_error() {
        let hub = WifiVideoHub::default();
        let (source, _) = hub.prepare("phone", (430., 932.)).await;
        let req = Request::delete(format!("/wifi-video/{}", source.id))
            .header("authorization", format!("Bearer {}", source.token))
            .body(Body::from("VideoToolbox encoder could not start"))
            .unwrap();
        assert_eq!(
            hub.router().oneshot(req).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        let error = source
            .ready(Duration::from_secs(1))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("VideoToolbox encoder could not start"),
            "{error}"
        );
    }
}

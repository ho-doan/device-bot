//! Android live inspection metadata and input. Video uses the WebRTC media gateway.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex as StdMutex},
    time::Duration,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use robot_core::{DeviceState, Platform, Point, Robot, RobotError, UiElement};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{AdminState, ErrorResponse, is_authorized};

const ELEMENT_GAP: Duration = Duration::from_millis(1200);

pub(crate) async fn android_live(
    Path(device_id): Path<String>,
    Query(query): Query<LiveQuery>,
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(max_frames) = query.max_frames {
        if !(1..=30).contains(&max_frames) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "maxFrames must be between 1 and 30".into(),
                }),
            )
                .into_response();
        }
    }
    if let Err(response) = require_android(&state, &device_id).await {
        return response;
    }
    state
        .activity_log
        .record("admin.live", device_id.clone())
        .await;

    let (sender, receiver) = mpsc::channel(4);
    let robot = Arc::clone(&state.robot);
    let max_frames = query.max_frames;
    tokio::spawn(async move {
        mirror(robot, device_id, sender, max_frames).await;
    });
    Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

pub(crate) async fn android_input(
    Path(device_id): Path<String>,
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(input): Json<LiveInput>,
) -> Response {
    if !is_authorized(&headers, state.bearer_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(response) = require_android(&state, &device_id).await {
        return response;
    }
    let outcome = {
        let gate = device_gate(&device_id);
        let _guard = gate.lock().await;
        dispatch_input(state.robot.as_ref(), &device_id, &input).await
    };
    match outcome {
        Ok(detail) => {
            state.activity_log.record("admin.input", detail).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            let status = match &error {
                RobotError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                RobotError::DeviceNotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::BAD_GATEWAY,
            };
            (
                status,
                Json(ErrorResponse {
                    error: error.to_string(),
                }),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveQuery {
    max_frames: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveInput {
    action: String,
    x: Option<f64>,
    y: Option<f64>,
    end_x: Option<f64>,
    end_y: Option<f64>,
    text: Option<String>,
    duration_ms: Option<u64>,
    mode: Option<String>,
    delta_x: Option<f64>,
    delta_y: Option<f64>,
}

async fn require_android(state: &AdminState, device_id: &str) -> Result<(), Response> {
    let devices = match state.robot.list_devices().await {
        Ok(devices) => devices,
        Err(error) => {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: error.to_string(),
                }),
            )
                .into_response());
        }
    };
    match devices.iter().find(|device| device.id == device_id) {
        Some(device)
            if device.platform == Platform::Android && device.state == DeviceState::Available =>
        {
            Ok(())
        }
        Some(device) if device.platform != Platform::Android => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Live mirror and live control are available for Android devices.".into(),
            }),
        )
            .into_response()),
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "The Android device is not available.".into(),
            }),
        )
            .into_response()),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("device not found: {device_id}"),
            }),
        )
            .into_response()),
    }
}

async fn dispatch_input(
    robot: &dyn Robot,
    device_id: &str,
    input: &LiveInput,
) -> Result<String, RobotError> {
    match input.action.as_str() {
        "tap" | "long_press" | "double_tap" => {
            let point = required_point(input.x, input.y)?;
            let gesture = robot_core::PointerGesture::parse(&input.action, input.duration_ms)?;
            robot
                .pointer_gesture(device_id, point, gesture, None)
                .await?;
            Ok(format!(
                "{device_id} {} {},{}",
                gesture.name(),
                point.x.round(),
                point.y.round()
            ))
        }
        "swipe" => {
            let start = required_point(input.x, input.y)?;
            let end = required_point(input.end_x, input.end_y)?;
            let duration_ms = input
                .duration_ms
                .unwrap_or(robot_core::DEFAULT_DRAG_DURATION_MS);
            match input.mode.as_deref() {
                Some("drag") => {
                    if !(1..=robot_core::MAX_DRAG_DURATION_MS).contains(&duration_ms)
                        || (start.x == end.x && start.y == end.y)
                    {
                        return Err(RobotError::InvalidInput(
                            "Drag requires distinct endpoints and durationMs between 1 and 3000"
                                .into(),
                        ));
                    }
                    robot.drag(device_id, start, end, duration_ms, None).await?;
                }
                None if (1..=60_000).contains(&duration_ms) => {
                    robot
                        .swipe(device_id, start, end, Some(duration_ms))
                        .await?;
                }
                None => {
                    return Err(RobotError::InvalidInput(
                        "durationMs must be between 1 and 60000".into(),
                    ));
                }
                _ => return Err(RobotError::InvalidInput("Unrecognized swipe mode".into())),
            }
            Ok(format!(
                "{device_id} swipe {},{} -> {},{}",
                start.x.round(),
                start.y.round(),
                end.x.round(),
                end.y.round()
            ))
        }
        "scroll" => {
            let at = required_point(input.x, input.y)?;
            let dx = input.delta_x.unwrap_or(0.);
            let dy = input.delta_y.unwrap_or(0.);
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 10000. || dy.abs() > 10000. {
                return Err(RobotError::InvalidInput(
                    "Invalid pixel scroll delta".into(),
                ));
            }
            robot.scroll(device_id, at, dx, dy).await?;
            Ok(format!("{device_id} scroll {dx},{dy}"))
        }
        "clear" => {
            robot.clear_text(device_id).await?;
            Ok(format!("{device_id} clear text"))
        }
        "type" => {
            let text = input.text.as_deref().unwrap_or("").trim();
            if text.is_empty() {
                return Err(RobotError::InvalidInput("text is required for type".into()));
            }
            if text.chars().count() > 500 {
                return Err(RobotError::InvalidInput(
                    "text must be 500 characters or fewer".into(),
                ));
            }
            robot.type_text(device_id, text).await?;
            Ok(format!("{device_id} type"))
        }
        _ => Err(RobotError::InvalidInput(
            "action must be tap, long_press, double_tap, swipe, scroll, type, or clear".into(),
        )),
    }
}

fn required_point(x: Option<f64>, y: Option<f64>) -> Result<Point, RobotError> {
    let (Some(x), Some(y)) = (x, y) else {
        return Err(RobotError::InvalidInput(
            "x and y coordinates are required".into(),
        ));
    };
    if !x.is_finite()
        || !y.is_finite()
        || !(0.0..=100_000.0).contains(&x)
        || !(0.0..=100_000.0).contains(&y)
    {
        return Err(RobotError::InvalidInput(
            "coordinates must be finite screen pixels".into(),
        ));
    }
    Ok(Point { x, y })
}

fn device_gate(device_id: &str) -> Arc<Mutex<()>> {
    static GATES: LazyLock<StdMutex<HashMap<String, Arc<Mutex<()>>>>> =
        LazyLock::new(|| StdMutex::new(HashMap::new()));
    let mut gates = GATES.lock().unwrap_or_else(|poison| poison.into_inner());
    gates
        .entry(device_id.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreferredTree {
    Auto,
    Flutter,
}

async fn mirror(
    robot: Arc<dyn Robot>,
    device_id: String,
    sender: mpsc::Sender<Result<Event, std::convert::Infallible>>,
    max_frames: Option<u32>,
) {
    let mut updates = 0;
    let mut preferred = PreferredTree::Auto;
    loop {
        let listed = tokio::select! {
            _ = sender.closed() => break,
            listed = async {
                let gate = device_gate(&device_id);
                let _guard = gate.lock().await;
                inspection_tree(robot.as_ref(), &device_id, &mut preferred).await
            } => listed,
        };
        let sent = match listed {
            Ok((elements, source)) => {
                send_json(
                    &sender,
                    "elements",
                    json!({"elements":elements,"source":source}),
                )
                .await
            }
            Err(error) => send_json(&sender, "error", json!({"error":error.to_string()})).await,
        };
        updates += 1;
        if sent.is_err() || max_frames.is_some_and(|limit| updates >= limit) {
            break;
        }
        tokio::select! { _ = sender.closed() => break, _ = tokio::time::sleep(ELEMENT_GAP) => {} }
    }
}

async fn inspection_tree(
    robot: &dyn Robot,
    device_id: &str,
    preferred: &mut PreferredTree,
) -> Result<(Vec<UiElement>, &'static str), RobotError> {
    if *preferred == PreferredTree::Flutter {
        match robot.list_flutter_elements(device_id).await {
            Ok(mut elements) if !elements.is_empty() => {
                robot_core::prepare_elements(&mut elements);
                return Ok((elements, "flutter"));
            }
            Ok(_) => *preferred = PreferredTree::Auto,
            Err(error) => {
                *preferred = PreferredTree::Auto;
                return Err(error);
            }
        }
    }

    let mut elements = robot.list_elements(device_id).await?;
    let mut source = "native";
    if elements.is_empty() || elements.iter().any(|element| !has_label(element)) {
        if let Ok(flutter) = robot.list_flutter_elements(device_id).await {
            if !flutter.is_empty() {
                elements = flutter;
                source = "flutter";
                *preferred = PreferredTree::Flutter;
            }
        }
    }
    robot_core::prepare_elements(&mut elements);
    Ok((elements, source))
}

fn has_label(element: &UiElement) -> bool {
    let present =
        |value: &Option<String>| value.as_deref().is_some_and(|text| !text.trim().is_empty());
    present(&element.text) || present(&element.content_description)
}

async fn send_json(
    sender: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    event: &str,
    payload: serde_json::Value,
) -> Result<(), mpsc::error::SendError<Result<Event, std::convert::Infallible>>> {
    sender
        .send(Ok(Event::default()
            .event(event)
            .json_data(payload)
            .expect("live event payload is JSON")))
        .await
}

#[cfg(test)]
mod tests;

//! Quartz drag gestures. Always release the button when cancelled or failed.
use robot_core::{Point, RobotError};
use std::time::Duration;

const MOVED: u32 = 5;
const DOWN: u32 = 1;
const UP: u32 = 2;
const DRAGGED: u32 = 6;

struct HeldPointer<F: FnMut(u32, Point) -> Result<(), RobotError>> {
    post: F,
    at: Point,
    held: bool,
}
impl<F: FnMut(u32, Point) -> Result<(), RobotError>> Drop for HeldPointer<F> {
    fn drop(&mut self) {
        if self.held {
            let _ = (self.post)(UP, self.at);
        }
    }
}

async fn drag_with<F: FnMut(u32, Point) -> Result<(), RobotError>>(
    start: Point,
    end: Point,
    duration_ms: u64,
    post: F,
) -> Result<(), RobotError> {
    if !(1..=60_000).contains(&duration_ms)
        || ![start.x, start.y, end.x, end.y]
            .into_iter()
            .all(f64::is_finite)
    {
        return Err(RobotError::InvalidInput(
            "Drag requires finite coordinates and durationMs 1..60000".into(),
        ));
    }
    let mut pointer = HeldPointer {
        post,
        at: start,
        held: false,
    };
    (pointer.post)(MOVED, start)?;
    (pointer.post)(DOWN, start)?;
    pointer.held = true;
    let steps = (duration_ms / 16).max(1);
    let delay = Duration::from_millis(duration_ms).div_f64(steps as f64);
    for index in 1..=steps {
        tokio::time::sleep(delay).await;
        let fraction = index as f64 / steps as f64;
        pointer.at = Point {
            x: start.x + (end.x - start.x) * fraction,
            y: start.y + (end.y - start.y) * fraction,
        };
        (pointer.post)(DRAGGED, pointer.at)?;
    }
    (pointer.post)(UP, end)?;
    pointer.held = false;
    Ok(())
}

#[cfg(target_os = "macos")]
struct PointerSource(super::CfOwned);
// CGEventSource has no UI-thread affinity. The drag owns it exclusively and
// posts one event at a time; it is never shared between concurrent tasks.
#[cfg(target_os = "macos")]
unsafe impl Send for PointerSource {}
#[cfg(target_os = "macos")]
impl PointerSource {
    fn post(&mut self, kind: u32, at: Point) -> Result<(), RobotError> {
        post(kind, at, self.0.0)
    }
}

#[cfg(target_os = "macos")]
pub(super) async fn drag(start: Point, end: Point, duration_ms: u64) -> Result<(), RobotError> {
    use super::{AXIsProcessTrusted, CFTypeRef, CfOwned};
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceCreate(state: i32) -> CFTypeRef;
        fn CGEventSourceSetLocalEventsSuppressionInterval(source: CFTypeRef, seconds: f64);
    }
    if !unsafe { AXIsProcessTrusted() } {
        return Err(RobotError::CommandFailed(
            "Grant Accessibility permission to Device Bot before dragging".into(),
        ));
    }
    // Keep synthetic button state separate from physical/remote desktop input.
    let source = CfOwned(unsafe { CGEventSourceCreate(-1) }); // kCGEventSourceStatePrivate
    if source.0.is_null() {
        return Err(RobotError::CommandFailed(
            "Could not create macOS pointer source".into(),
        ));
    }
    unsafe { CGEventSourceSetLocalEventsSuppressionInterval(source.0, 0.0) };
    let mut source = PointerSource(source);
    drag_with(start, end, duration_ms, move |kind, at| {
        source.post(kind, at)
    })
    .await
}

#[cfg(target_os = "macos")]
fn post(kind: u32, at: Point, source: super::CFTypeRef) -> Result<(), RobotError> {
    use super::{CFTypeRef, CGPoint, CfOwned};
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateMouseEvent(
            source: CFTypeRef,
            kind: u32,
            at: CGPoint,
            button: u32,
        ) -> CFTypeRef;
        fn CGEventSetFlags(event: CFTypeRef, flags: u64);
        fn CGEventSetIntegerValueField(event: CFTypeRef, field: u32, value: i64);
        fn CGEventPost(tap: u32, event: CFTypeRef);
    }
    let event =
        CfOwned(unsafe { CGEventCreateMouseEvent(source, kind, CGPoint { x: at.x, y: at.y }, 0) });
    if event.0.is_null() {
        return Err(RobotError::CommandFailed(
            "Could not create macOS pointer event".into(),
        ));
    }
    unsafe {
        CGEventSetFlags(event.0, 0);
        CGEventSetIntegerValueField(event.0, 1, 1); // kCGMouseEventClickState
        CGEventPost(1, event.0); // kCGSessionEventTap
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) async fn drag(_start: Point, _end: Point, _duration_ms: u64) -> Result<(), RobotError> {
    Err(RobotError::Unsupported(
        "macOS pointer events require macOS".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn drag_moves_to_endpoint_and_releases() {
        let mut events = Vec::new();
        drag_with(
            Point { x: 10., y: 20. },
            Point { x: 40., y: 80. },
            32,
            |kind, point| {
                events.push((kind, point.x, point.y));
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(events.first(), Some(&(MOVED, 10., 20.)));
        assert_eq!(events.get(1), Some(&(DOWN, 10., 20.)));
        assert_eq!(events.last(), Some(&(UP, 40., 80.)));
        assert!(events.iter().any(|event| event.0 == DRAGGED));
    }
    #[tokio::test]
    async fn cancellation_releases_without_finishing_drag() {
        let mut events = Vec::new();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(2),
                drag_with(
                    Point { x: 1., y: 2. },
                    Point { x: 10., y: 20. },
                    200,
                    |kind, point| {
                        events.push((kind, point.x, point.y));
                        Ok(())
                    }
                )
            )
            .await
            .is_err()
        );
        assert_eq!(events, [(MOVED, 1., 2.), (DOWN, 1., 2.), (UP, 1., 2.)]);
    }
    #[tokio::test]
    async fn invalid_input_never_presses_mouse() {
        let result = drag_with(
            Point { x: f64::NAN, y: 2. },
            Point { x: 10., y: 20. },
            300,
            |_, _| panic!("must not post invalid coordinates"),
        )
        .await;
        assert!(matches!(result, Err(RobotError::InvalidInput(_))));
    }
}

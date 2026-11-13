//! Quartz drag gestures. Always release the button when cancelled or failed.
use robot_core::{Point, RobotError};
use std::time::Duration;

type Post = fn(u32, Point) -> Result<(), RobotError>;
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

pub(super) async fn drag(start: Point, end: Point, duration_ms: u64) -> Result<(), RobotError> {
    drag_with(start, end, duration_ms, post as Post).await
}

#[cfg(target_os = "macos")]
fn post(kind: u32, at: Point) -> Result<(), RobotError> {
    use super::{AXIsProcessTrusted, CFTypeRef, CGPoint, CfOwned};
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateMouseEvent(
            source: CFTypeRef,
            kind: u32,
            at: CGPoint,
            button: u32,
        ) -> CFTypeRef;
        fn CGEventSetFlags(event: CFTypeRef, flags: u64);
        fn CGEventPost(tap: u32, event: CFTypeRef);
    }
    if !unsafe { AXIsProcessTrusted() } {
        return Err(RobotError::CommandFailed(
            "Grant Accessibility permission to Device Bot before dragging".into(),
        ));
    }
    let event = CfOwned(unsafe {
        CGEventCreateMouseEvent(std::ptr::null(), kind, CGPoint { x: at.x, y: at.y }, 0)
    });
    if event.0.is_null() {
        return Err(RobotError::CommandFailed(
            "Could not create macOS pointer event".into(),
        ));
    }
    unsafe {
        CGEventSetFlags(event.0, 0);
        CGEventPost(0, event.0);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn post(_kind: u32, _at: Point) -> Result<(), RobotError> {
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
        assert_eq!(events.first(), Some(&(DOWN, 10., 20.)));
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
        assert_eq!(events, [(DOWN, 1., 2.), (UP, 1., 2.)]);
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

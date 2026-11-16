//! Platform-neutral device contracts and driver routing.
//!
//! Public contracts are re-exported here so drivers and transports do not depend
//! on the internal module layout. Query policy lives beside accessibility models;
//! session ownership and device selection live in the registry.

mod device;
mod elements;
mod error;
mod input;
mod observation;
mod registry;
mod robot;
mod web;

pub mod health;
pub mod video;

pub use device::{
    AppWindow, Bounds, DesktopDisplay, DeviceState, Platform, Point, RobotDevice, RunningApp,
    ScreenshotOptions, UiTarget,
};
pub use elements::{
    ElementQuery, ElementType, UiElement, apply_element_query, classify_class_name,
    element_matches_filter, prepare_elements,
};
pub use error::RobotError;
pub use input::{ImeAction, UiAction};
pub use observation::{ObservationBatch, ObservationOptions, ObservationRecord};
pub use registry::RobotRegistry;
pub use robot::{Robot, UiRunnerStatus};
pub use video::{NativeVideoStream, VideoFrame, VideoStream};
pub use web::{WebContext, WebFrameSelector, WebPageSelector, web_context_matches};

#[cfg(test)]
mod tests;

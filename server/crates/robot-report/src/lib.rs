//! Persistent QA report service. Artifact snapshots are independent of the runner.
pub mod http;
pub mod model;
pub mod store;
pub mod targets;
pub use http::{index_page, router};
pub use store::{FlowIngest, Store, default_reports_dir};

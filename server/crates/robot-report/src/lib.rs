//! Persistent QA report service. Artifact snapshots are independent of the runner.
pub mod http;
pub mod model;
pub mod store;
pub use http::router;
pub use store::Store;

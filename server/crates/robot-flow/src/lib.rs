//! Declarative scenarios executed exclusively through the device-bot MCP server.
pub mod mcp;
pub mod runner;
pub mod schema;
mod trace;
pub use runner::{FlowReport, FlowRunner};
pub use schema::{FlowDocument, Profile, Profiles};

pub mod http_mcp;

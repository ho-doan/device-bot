//! MCP server composition.
//!
//! - [`server`] owns request routing and the connection state machine.
//! - [`lifecycle`] owns only MCP lifecycle transitions.
//! - [`response`] owns JSON-RPC response encoding.
//! - [`stdio`] owns newline-delimited stdio I/O.

pub mod http;
mod lifecycle;
mod response;
mod server;
pub mod stdio;
mod tools;

pub use server::Server;

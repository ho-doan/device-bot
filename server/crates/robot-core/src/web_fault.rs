//! Typed controls for a disposable browser session's test proxy.

use crate::RobotError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebProxyConfig {
    pub upstream: SocketAddr,
    /// Other local origins this browser session may reach without injected faults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passthrough: Vec<SocketAddr>,
}

impl WebProxyConfig {
    pub fn validate(&self) -> Result<(), RobotError> {
        if self.passthrough.len() > 8
            || std::iter::once(&self.upstream)
                .chain(&self.passthrough)
                .any(|addr| !addr.ip().is_loopback() || addr.port() == 0)
        {
            return Err(RobotError::InvalidInput(
                "web proxy requires explicit loopback addresses with ports, with at most eight passthrough origins".into(),
            ));
        }
        if self
            .passthrough
            .iter()
            .enumerate()
            .any(|(index, addr)| *addr == self.upstream || self.passthrough[..index].contains(addr))
        {
            return Err(RobotError::InvalidInput(
                "web proxy origins must be distinct".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebFault {
    HttpStatus { path: String, status: u16 },
    Delay { path: String, ms: u64 },
    Drop { path: String },
    WebSocketReject,
    WebSocketDrop,
}

impl WebFault {
    pub fn validate(&self) -> Result<(), RobotError> {
        if self.http_path().is_some_and(|path| {
            !path.starts_with('/')
                || path.len() > 1024
                || !path
                    .bytes()
                    .all(|n| n.is_ascii_graphic() && !matches!(n, b'?' | b'#'))
        }) {
            return Err(RobotError::InvalidInput(
                "web fault path must be an absolute URL path without query or fragment".into(),
            ));
        }
        match self {
            Self::HttpStatus { status, .. } if !(400..=599).contains(status) => Err(
                RobotError::InvalidInput("web fault status must be 400..599".into()),
            ),
            Self::Delay { ms, .. } if !(1..=30_000).contains(ms) => Err(RobotError::InvalidInput(
                "web fault delay must be 1..30000 ms".into(),
            )),
            _ => Ok(()),
        }
    }

    pub fn http_path(&self) -> Option<&str> {
        match self {
            Self::HttpStatus { path, .. } | Self::Delay { path, .. } | Self::Drop { path } => {
                Some(path)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFaultState {
    pub revision: u64,
    pub fault: Option<WebFault>,
    pub hits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFaultTransition {
    pub previous: WebFaultState,
    pub active: WebFaultState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_test_scope_and_fault_bounds_are_enforced() {
        for addr in ["0.0.0.0:8080", "192.0.2.1:8080", "127.0.0.1:0"] {
            let config = WebProxyConfig {
                upstream: addr.parse().unwrap(),
                passthrough: vec![],
            };
            assert!(config.validate().is_err(), "{addr}");
        }
        let upstream = "127.0.0.1:8080".parse().unwrap();
        assert!(
            WebProxyConfig {
                upstream,
                passthrough: vec![upstream]
            }
            .validate()
            .is_err()
        );
        for path in ["data", "/data?token=value", "/data#fragment", "/a b"] {
            assert!(
                WebFault::Drop { path: path.into() }.validate().is_err(),
                "{path}"
            );
        }
        assert!(
            WebFault::HttpStatus {
                path: "/data".into(),
                status: 200
            }
            .validate()
            .is_err()
        );
        assert!(
            WebFault::Delay {
                path: "/data".into(),
                ms: 30_001
            }
            .validate()
            .is_err()
        );
        assert!(
            WebFault::HttpStatus {
                path: "/data".into(),
                status: 503
            }
            .validate()
            .is_ok()
        );
    }
}

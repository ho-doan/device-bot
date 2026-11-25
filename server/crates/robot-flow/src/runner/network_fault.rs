//! Web fault step execution and report metadata. Network I/O stays in the driver.

use super::FlowRunner;
use crate::{
    mcp::{Result, WEB_TEST_PROXY},
    outcome::{ErrorKind, FlowError},
    schema::{self, NetworkFaultStep},
};
use robot_core::{WebFault, WebFaultState, WebFaultTransition};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct NetworkFaultReport {
    pub operation: &'static str,
    pub elapsed_ms: u64,
    pub active: NetworkFaultSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<NetworkFaultSnapshot>,
}

/// No URLs, path values, request headers, or payloads enter the evidence.
#[derive(Clone, Debug, Serialize)]
pub struct NetworkFaultSnapshot {
    pub revision: u64,
    pub kind: Option<&'static str>,
    pub hits: u64,
}

impl From<WebFaultState> for NetworkFaultSnapshot {
    fn from(state: WebFaultState) -> Self {
        Self {
            revision: state.revision,
            hits: state.hits,
            kind: state.fault.map(|fault| match fault {
                WebFault::HttpStatus { .. } => "http_status",
                WebFault::Delay { .. } => "delay",
                WebFault::Drop { .. } => "drop",
                WebFault::WebSocketReject => "web_socket_reject",
                WebFault::WebSocketDrop => "web_socket_drop",
            }),
        }
    }
}

fn state_from<T: DeserializeOwned>(result: &Value) -> Result<T> {
    serde_json::from_value(result["structuredContent"].clone())
        .map_err(|_| FlowError::from("invalid web test proxy status").into_transport())
}

impl FlowRunner {
    pub(super) async fn run_web_fault(&mut self, args: &Value) -> Result<NetworkFaultReport> {
        let step = schema::network_fault_step(args)?;
        let device = self.device_id()?.to_owned();
        if matches!(step, NetworkFaultStep::Set { .. }) {
            // A cancelled RPC may have applied the fault without a reply.
            self.owned_faults.insert(device.clone());
        }
        let result = self
            .call(WEB_TEST_PROXY, step.tool_arguments(&device))
            .await?;
        let (operation, active, previous) = match step {
            NetworkFaultStep::Set { fault } => {
                let change: WebFaultTransition = state_from(&result)?;
                if change.active.fault.as_ref() != Some(&fault) {
                    return Err("test proxy did not confirm the requested fault".into());
                }
                ("set", change.active, Some(change.previous))
            }
            NetworkFaultStep::Status { min_hits } => {
                let state: WebFaultState = state_from(&result)?;
                if let Some(minimum) = min_hits
                    && state.hits < minimum
                {
                    return Err(FlowError::new(
                        ErrorKind::Assertion,
                        format!(
                            "expected at least {minimum} fault hit(s), observed {}",
                            state.hits
                        ),
                    ));
                }
                ("status", state, None)
            }
            NetworkFaultStep::Clear {} => {
                let change = cleared(&result)?;
                self.owned_faults.remove(&device);
                ("clear", change.active, Some(change.previous))
            }
        };
        Ok(NetworkFaultReport {
            operation,
            elapsed_ms: self.run_started.elapsed().as_millis() as u64,
            active: active.into(),
            previous: previous.map(Into::into),
        })
    }

    pub(super) async fn clear_owned_faults(&mut self) -> Result<()> {
        let mut first = None;
        for device in std::mem::take(&mut self.owned_faults) {
            let result = self
                .call(
                    WEB_TEST_PROXY,
                    (NetworkFaultStep::Clear {}).tool_arguments(&device),
                )
                .await
                .and_then(|value| cleared(&value));
            if let Err(error) = result {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

fn cleared(result: &Value) -> Result<WebFaultTransition> {
    let change: WebFaultTransition = state_from(result)?;
    if change.active.fault.is_some() {
        return Err("test proxy did not confirm fault clearance".into());
    }
    Ok(change)
}

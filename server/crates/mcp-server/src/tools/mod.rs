//! MCP tool catalogue and family-level dispatch.

mod app;
mod arguments;
mod device;
mod input;
mod navigation;
mod screen;

use std::sync::Arc;

use mobile_mcp_protocol::{CallToolParams, ToolDefinition, ToolResult};
use mobile_robot_core::Robot;

pub(crate) struct ToolRegistry {
    robot: Arc<dyn Robot>,
}

impl ToolRegistry {
    pub(crate) fn new(robot: Arc<dyn Robot>) -> Self {
        Self { robot }
    }

    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![
            device::definition(),
            app::definition(),
            navigation::definition(),
            input::definition(),
        ];
        definitions.extend(screen::definitions());
        definitions
    }

    pub(crate) async fn call(
        &self,
        params: CallToolParams,
    ) -> Result<ToolResult, mobile_mcp_protocol::RpcError> {
        let result = match params.name.as_str() {
            device::NAME => device::call(&self.robot).await,
            app::LAUNCH_NAME => app::launch(&self.robot, &params.arguments).await,
            navigation::OPEN_URL_NAME => navigation::open_url(&self.robot, &params.arguments).await,
            input::TYPE_KEYS_NAME => input::type_keys(&self.robot, &params.arguments).await,
            screen::SCREENSHOT_NAME => screen::screenshot(&self.robot, &params.arguments).await,
            screen::TAP_NAME => screen::tap(&self.robot, &params.arguments).await,
            name => {
                return Err(mobile_mcp_protocol::RpcError::invalid_params(format!(
                    "Unknown tool: {name}"
                )));
            }
        };
        Ok(result)
    }
}

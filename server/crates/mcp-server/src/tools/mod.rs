//! MCP tool catalogue and family-level dispatch.

mod app;
mod apps;
mod arguments;
mod batch;
mod device;
mod input;
mod navigation;
mod recording;
mod runner_status;
mod screen;
mod ui;

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
            device::definitions()[0].clone(),
            device::definitions()[1].clone(),
            navigation::definition(),
            input::definition(),
            ui::definition(),
            batch::definition(),
            runner_status::definition(),
        ];
        definitions.extend(app::definitions());
        definitions.push(apps::definition());
        definitions.extend(recording::definitions());
        definitions.extend(screen::definitions());
        definitions
    }

    pub(crate) async fn call(
        &self,
        params: CallToolParams,
    ) -> Result<ToolResult, mobile_mcp_protocol::RpcError> {
        let result = match params.name.as_str() {
            device::NAME => device::call(&self.robot).await,
            device::BOOT_NAME => device::boot(&self.robot, &params.arguments).await,
            recording::START_NAME => recording::start(&self.robot, &params.arguments).await,
            recording::STOP_NAME => recording::stop(&self.robot, &params.arguments).await,
            app::LAUNCH_NAME => app::launch(&self.robot, &params.arguments).await,
            app::INSTALL_NAME => app::install(&self.robot, &params.arguments).await,
            app::UNINSTALL_NAME => app::uninstall(&self.robot, &params.arguments).await,
            apps::LIST_INSTALLED_APPS_NAME => apps::list_apps(&self.robot, &params.arguments).await,
            navigation::OPEN_URL_NAME => navigation::open_url(&self.robot, &params.arguments).await,
            input::TYPE_KEYS_NAME => input::type_keys(&self.robot, &params.arguments).await,
            screen::SCREENSHOT_NAME => screen::screenshot(&self.robot, &params.arguments).await,
            screen::TAP_NAME => screen::tap(&self.robot, &params.arguments).await,
            screen::SWIPE_NAME => screen::swipe(&self.robot, &params.arguments).await,
            ui::LIST_ELEMENTS_NAME => ui::list_elements(&self.robot, &params.arguments).await,
            batch::NAME => batch::execute(&self.robot, &params.arguments).await,
            runner_status::NAME => runner_status::get(&self.robot, &params.arguments).await,
            name => {
                return Err(mobile_mcp_protocol::RpcError::invalid_params(format!(
                    "Unknown tool: {name}"
                )));
            }
        };
        Ok(result)
    }
}

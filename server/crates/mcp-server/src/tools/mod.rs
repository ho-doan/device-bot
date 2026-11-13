//! MCP tool catalogue and family-level dispatch.

mod app;
mod apps;
mod arguments;
mod batch;
mod device;
mod flow;
mod input;
mod navigation;
mod recording;
mod runner_status;
mod screen;
mod stream;
mod ui;

use std::sync::Arc;

use device_mcp_protocol::{CallToolParams, ToolContent, ToolDefinition, ToolResult};
use robot_core::Robot;

pub(crate) struct ToolRegistry {
    robot: Arc<dyn Robot>,
    streams: robot_stream::StreamManager,
}

impl ToolRegistry {
    pub(crate) fn new(robot: Arc<dyn Robot>) -> Self {
        Self {
            robot,
            streams: robot_stream::StreamManager::default(),
        }
    }

    pub(crate) async fn stop_streams(&self) {
        self.streams.stop_all().await;
    }

    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![
            device::definitions()[0].clone(),
            device::definitions()[1].clone(),
            navigation::definition(),
            input::definition(),
            input::press_ime_definition(),
            ui::definition(),
            batch::definition(),
            runner_status::definition(),
        ];
        definitions.extend(app::definitions());
        definitions.extend(apps::definitions());
        definitions.extend(recording::definitions());
        definitions.extend(stream::definitions());
        definitions.extend(screen::definitions());
        definitions.push(flow::definition());
        definitions
    }

    pub(crate) async fn call(
        &self,
        params: CallToolParams,
    ) -> Result<ToolResult, device_mcp_protocol::RpcError> {
        let mut result = match params.name.as_str() {
            flow::NAME => Box::pin(flow::run(&self.robot, &params.arguments)).await,
            stream::START_NAME => {
                stream::start(&self.robot, &self.streams, &params.arguments).await
            }
            stream::STOP_NAME => stream::stop(&self.streams, &params.arguments).await,
            device::NAME => device::call(&self.robot).await,
            device::BOOT_NAME => device::boot(&self.robot, &params.arguments).await,
            recording::START_NAME => recording::start(&self.robot, &params.arguments).await,
            recording::STOP_NAME => recording::stop(&self.robot, &params.arguments).await,
            app::LAUNCH_NAME => app::launch(&self.robot, &params.arguments).await,
            app::TERMINATE_NAME => app::terminate(&self.robot, &params.arguments).await,
            app::TERMINATE_ALL_NAME => app::terminate_all(&self.robot, &params.arguments).await,
            app::INSTALL_NAME => app::install(&self.robot, &params.arguments).await,
            app::UNINSTALL_NAME => app::uninstall(&self.robot, &params.arguments).await,
            apps::LIST_INSTALLED_APPS_NAME => apps::list_apps(&self.robot, &params.arguments).await,
            apps::LIST_RUNNING_APPS_NAME => {
                apps::list_running_apps(&self.robot, &params.arguments).await
            }
            apps::LIST_WINDOWS_NAME => apps::list_windows(&self.robot, &params.arguments).await,
            apps::ACTIVATE_APP_NAME => apps::activate_app(&self.robot, &params.arguments).await,
            navigation::OPEN_URL_NAME => navigation::open_url(&self.robot, &params.arguments).await,
            input::TYPE_KEYS_NAME => input::type_keys(&self.robot, &params.arguments).await,
            input::PRESS_IME_ACTION_NAME => {
                input::press_ime_action(&self.robot, &params.arguments).await
            }
            screen::SCREENSHOT_NAME => screen::screenshot(&self.robot, &params.arguments).await,
            screen::DISPLAYS_NAME => screen::displays(&self.robot, &params.arguments).await,
            screen::TAP_NAME => screen::tap(&self.robot, &params.arguments).await,
            screen::SCROLL_NAME => screen::scroll(&self.robot, &params.arguments).await,
            screen::SWIPE_NAME => screen::swipe(&self.robot, &params.arguments).await,
            ui::LIST_ELEMENTS_NAME => ui::list_elements(&self.robot, &params.arguments).await,
            batch::NAME => batch::execute(&self.robot, &params.arguments).await,
            runner_status::NAME => runner_status::get(&self.robot, &params.arguments).await,
            name => {
                return Err(device_mcp_protocol::RpcError::invalid_params(format!(
                    "Unknown tool: {name}"
                )));
            }
        };
        if result.is_error == Some(true) {
            let locked = result.content.iter().find_map(|content| match content {
                ToolContent::Text { text } => text.strip_prefix("device_locked:").map(str::trim),
                _ => None,
            });
            if let Some(detail) = locked {
                result.structured_content = Some(serde_json::json!({
                    "state":"device_locked", "code":"device_locked", "ready":false,
                    "deviceId":params.arguments["deviceId"], "detail":detail,
                    "recommendedActions":["unlock_device", "retry_operation"],
                    "diagnostics":{"connected":true,"screenLocked":true}
                }));
            }
        }
        Ok(result)
    }
}

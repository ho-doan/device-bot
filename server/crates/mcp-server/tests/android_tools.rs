use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use device_bot::Server;
use device_mcp_protocol::{JsonRpcRequest, PROTOCOL_VERSION};
use robot_core::{
    AppWindow, Bounds, DeviceState, ElementType, ImeAction, Platform, Point, Robot, RobotDevice,
    RobotError, RunningApp, UiElement, UiTarget,
};
use serde_json::{Value, json};

#[derive(Default)]
struct RecordingRobot {
    taps: Mutex<Vec<(String, Point)>>,
    actions: Mutex<Vec<String>>,
    running_apps: Mutex<Vec<RunningApp>>,
    windows: Mutex<Vec<AppWindow>>,
    selected: Mutex<Vec<String>>,
    recording_files: Mutex<Vec<PathBuf>>,
    query_delay: std::time::Duration,
}

#[async_trait]
impl Robot for RecordingRobot {
    async fn capabilities(&self, id: &str) -> Result<robot_core::CapabilityReport, RobotError> {
        if id == "failure" {
            return Err(RobotError::CommandFailed("secret-token-from-driver".into()));
        }
        Ok(robot_core::CapabilityReport::new(id))
    }

    async fn list_devices(&self) -> Result<Vec<RobotDevice>, RobotError> {
        Ok(vec![RobotDevice {
            id: "emulator-5554".into(),
            name: "Pixel 9".into(),
            platform: Platform::Android,
            state: DeviceState::Available,
        }])
    }

    async fn list_installed_apps(&self, device_id: &str) -> Result<Value, RobotError> {
        assert_eq!(device_id, "emulator-5554");
        Ok(json!({
            "platform": "android",
            "deviceId": device_id,
            "apps": [],
        }))
    }

    async fn list_running_apps(&self, device_id: &str) -> Result<Vec<RunningApp>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("running:{device_id}"));
        Ok(self.running_apps.lock().unwrap().clone())
    }

    async fn list_app_windows(
        &self,
        device_id: &str,
        process_id: Option<u32>,
    ) -> Result<Vec<AppWindow>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("windows:{device_id}:{}", process_id.unwrap_or(0)));
        Ok(self
            .windows
            .lock()
            .unwrap()
            .iter()
            .filter(|window| process_id.is_none_or(|id| window.process_id == id))
            .cloned()
            .collect())
    }

    async fn select_ui_target(&self, device_id: &str, target: UiTarget) -> Result<(), RobotError> {
        self.selected.lock().unwrap().push(format!(
            "{device_id}:{}:{}",
            target.process_id.unwrap_or(0),
            target.window_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn activate_ui_target(
        &self,
        device_id: &str,
        target: UiTarget,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "activate:{device_id}:{}:{}",
            target.process_id.unwrap_or(0),
            target.window_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn take_screenshot(&self, device_id: &str) -> Result<Vec<u8>, RobotError> {
        if device_id == "locked" {
            return Err(RobotError::DeviceLocked(device_id.into()));
        }
        if device_id == "emulator-5554" {
            Ok(vec![1, 2, 3])
        } else {
            Err(RobotError::DeviceNotFound(device_id.into()))
        }
    }

    async fn start_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("record-start:{device_id}"));
        Ok(format!("artifacts/{device_id}.mp4"))
    }

    async fn stop_screen_recording(&self, device_id: &str) -> Result<String, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("record-stop:{device_id}"));
        let path = std::env::temp_dir().join(format!(
            "device-bot-recording-test-{}.mp4",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::write(&path, [1, 2, 3])
            .await
            .map_err(|error| RobotError::CommandFailed(error.to_string()))?;
        self.recording_files.lock().unwrap().push(path.clone());
        Ok(path.to_string_lossy().into_owned())
    }

    async fn tap(&self, device_id: &str, point: Point) -> Result<(), RobotError> {
        self.taps.lock().unwrap().push((device_id.into(), point));
        Ok(())
    }

    async fn launch_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("launch:{device_id}:{app_id}"));
        Ok(())
    }

    async fn stream_context(&self, _: &str) -> Result<Value, RobotError> {
        Ok(json!({"appId":"com.example.foreground"}))
    }

    async fn terminate_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("terminate:{device_id}:{app_id}"));
        Ok(())
    }

    async fn terminate_all_apps(&self, device_id: &str) -> Result<Vec<String>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("terminate-all:{device_id}"));
        Ok(vec!["com.example.foreground".into()])
    }

    async fn open_url(
        &self,
        device_id: &str,
        url: &str,
        app_id: Option<&str>,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "open:{device_id}:{url}:{}",
            app_id.unwrap_or_default()
        ));
        Ok(())
    }

    async fn type_text(&self, device_id: &str, text: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("type:{device_id}:{text}"));
        Ok(())
    }

    async fn press_ime_action(&self, device_id: &str, action: ImeAction) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("ime:{device_id}:{}", action.as_str()));
        Ok(())
    }

    async fn swipe(
        &self,
        device_id: &str,
        start: Point,
        end: Point,
        duration_ms: Option<u64>,
    ) -> Result<(), RobotError> {
        self.actions.lock().unwrap().push(format!(
            "swipe:{device_id}:{},{},{},{}:{}",
            start.x,
            start.y,
            end.x,
            end.y,
            duration_ms.unwrap_or(300)
        ));
        Ok(())
    }

    async fn install_app(&self, device_id: &str, apk_path: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("install:{device_id}:{apk_path}"));
        Ok(())
    }

    async fn uninstall_app(&self, device_id: &str, app_id: &str) -> Result<(), RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push(format!("uninstall:{device_id}:{app_id}"));
        Ok(())
    }

    async fn list_flutter_elements(&self, _device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        self.actions
            .lock()
            .unwrap()
            .push("flutter-discovery".into());
        let mut node = self.list_elements("emulator-5554").await?.remove(0);
        node.resource_id = Some("flutter-only-key".into());
        node.text = Some("Flutter clear".into());
        node.value = Some(String::new());
        Ok(vec![node])
    }

    async fn list_elements(&self, device_id: &str) -> Result<Vec<UiElement>, RobotError> {
        tokio::time::sleep(self.query_delay).await;
        if device_id == "native-error" {
            return Err(RobotError::CommandFailed("native discovery failed".into()));
        }
        Ok(vec![
            UiElement {
                class_name: "android.widget.Button".into(),
                resource_id: Some("com.example:id/submit".into()),
                text: Some("Submit".into()),
                content_description: None,
                bounds: Bounds {
                    left: 10,
                    top: 20,
                    right: 100,
                    bottom: 70,
                },
                clickable: true,
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value: None,
                element_type: None,
                web_context: None,
            },
            UiElement {
                class_name: "android.widget.Button".into(),
                resource_id: None,
                text: Some("Cancel".into()),
                content_description: None,
                bounds: Bounds {
                    left: 110,
                    top: 20,
                    right: 200,
                    bottom: 70,
                },
                clickable: true,
                enabled: true,
                focused: None,
                checked: None,
                selected: None,
                value: None,
                element_type: None,
                web_context: None,
            },
            UiElement {
                class_name: "android.widget.EditText".into(),
                resource_id: Some("com.example:id/password".into()),
                text: Some("secret".into()),
                content_description: None,
                bounds: Bounds {
                    left: 10,
                    top: 80,
                    right: 200,
                    bottom: 120,
                },
                clickable: true,
                enabled: true,
                focused: Some(true),
                checked: None,
                selected: None,
                value: Some("secret".into()),
                element_type: Some(ElementType::TextField),
                web_context: None,
            },
        ])
    }
}

fn request(id: u64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(id)),
        method: method.into(),
        params: Some(params),
    }
}

async fn ready(server: &mut Server) {
    server
        .handle(request(
            1,
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "0.1.0" }
            }),
        ))
        .await;
    server
        .handle(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        })
        .await;
}

struct RunnerMcp(Server);

#[async_trait]
impl robot_flow::mcp::Mcp for RunnerMcp {
    async fn request(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<Value> {
        let response = self
            .0
            .handle(request(1, method, params))
            .await
            .ok_or("missing MCP response")?;
        if let Some(error) = response.get("error") {
            return Err(error.to_string().into());
        }
        response
            .get("result")
            .cloned()
            .ok_or("missing MCP result".into())
    }

    async fn notify(&mut self, method: &str, params: Value) -> robot_flow::mcp::Result<()> {
        self.0
            .handle(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: method.into(),
                params: Some(params),
            })
            .await;
        Ok(())
    }

    async fn close(&mut self) -> robot_flow::mcp::Result<()> {
        self.0
            .shutdown()
            .await
            .map_err(|error| error.to_string().into())
    }
}

#[path = "android_tools/commands.rs"]
mod commands;
#[path = "android_tools/deadlines.rs"]
mod deadlines;
#[path = "android_tools/inspection.rs"]
mod inspection;

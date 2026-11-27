//! Resolves the application, window, or browser origin under exploration.

use super::{Options, Scope, clipped};
use robot_core::{DeviceState, Platform, Robot};
use url::Url;

pub(super) fn parse_web(value: &str) -> Result<Url, String> {
    let url = Url::parse(value)
        .map_err(|_| "Open an HTTP(S) page in the selected Chrome tab first.".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("UI mapping requires an HTTP(S) tab URL without embedded credentials.".into());
    }
    Ok(url)
}

pub(super) fn web_location(url: &Url) -> String {
    let mut location = format!("{}{}", url.origin().ascii_serialization(), url.path());
    if let Some(route) = url.fragment().filter(|f| f.starts_with('/')) {
        location.push('#');
        location.push_str(route.split('?').next().unwrap_or_default());
    }
    clipped(&location, 240)
}

pub(super) async fn foreground_app(
    robot: &dyn Robot,
    device: &str,
    platform: &Platform,
) -> Result<Option<String>, String> {
    if *platform == Platform::Ios {
        let status = robot
            .ui_runner_status(device)
            .await
            .map_err(|error| error.to_string())?;
        if !status.ready {
            return Err(format!(
                "Could not verify the iOS foreground app: {}",
                status.detail
            ));
        }
        return Ok(status.target_app_id);
    }
    let context = robot
        .stream_context(device)
        .await
        .map_err(|error| error.to_string())?;
    Ok(context["appId"]
        .as_str()
        .filter(|app| !app.is_empty())
        .map(str::to_owned))
}

pub(super) async fn classify_scope(robot: &dyn Robot, opt: &Options) -> Result<Scope, String> {
    let device = &opt.device_id;
    if device.starts_with("chrome:") {
        if opt.reset_app_id.is_some() {
            return Err("Chrome restores the entry URL and does not use resetAppId.".into());
        }
        let context = robot
            .stream_context(device)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(Scope::Web {
            start: parse_web(context["url"].as_str().unwrap_or_default())?,
        });
    }
    if ["windows:", "macos:"]
        .iter()
        .any(|prefix| device.starts_with(prefix))
    {
        if device.split(':').count() != 3 {
            return Err("Select a desktop process and window before collecting a map.".into());
        }
        if let Some(app) = &opt.reset_app_id {
            if !device.starts_with("macos:") {
                return Err("resetAppId is only supported for macOS and mobile fixtures.".into());
            }
            robot
                .bind_window_fixture(device, app)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(Scope::WindowFixture);
        }
        return Ok(Scope::Native);
    }
    let devices = robot.list_devices().await.map_err(|e| e.to_string())?;
    let target = devices
        .iter()
        .find(|d| d.id == *device && d.state == DeviceState::Available)
        .ok_or("Selected device is unavailable.")?;
    match &target.platform {
        platform @ (Platform::Android | Platform::Ios)
            if *platform == Platform::Android || opt.reset_app_id.is_some() =>
        {
            let app = foreground_app(robot, device, platform)
                .await?
                .ok_or("Open the mobile app before mapping.")?;
            if opt
                .reset_app_id
                .as_deref()
                .is_some_and(|reset| reset != app)
            {
                return Err("resetAppId must match the currently open app.".into());
            }
            Ok(Scope::Mobile {
                app,
                platform: platform.clone(),
            })
        }
        Platform::Ios => Ok(Scope::IosNative),
        _ => Err("Select a specific Chrome tab or desktop window.".into()),
    }
}

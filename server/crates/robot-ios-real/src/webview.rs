//! Optional WebKit inspect helper for in-app WebViews.
//!
//! An explicit web context calls this helper and never falls back to XCTest.
//! A missing helper leaves unscoped native queries on the accessibility driver.

use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use robot_core::{Bounds, ElementType, RobotError, UiElement, WebContext};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

pub async fn read_dom(
    udid: &str,
    bundle_id: &str,
    transport: &str,
    context: &WebContext,
) -> Result<Vec<UiElement>, RobotError> {
    let response = exchange(
        &helper_script()?,
        &json!({
            "udid": udid,
            "bundleId": bundle_id,
            "transport": transport,
            "action": "list",
            "webContext": context,
            "pageUrl": context.page.as_ref().and_then(|page| page.url.clone()),
            "pageTitle": context.page.as_ref().and_then(|page| page.title.clone()),
        }),
    )
    .await?;
    decode_elements(&response)
}

pub async fn edit_dom(
    udid: &str,
    bundle_id: &str,
    transport: &str,
    context: &WebContext,
    action: &str,
    text: &str,
) -> Result<(), RobotError> {
    let response = exchange(
        &helper_script()?,
        &json!({
            "udid": udid,
            "bundleId": bundle_id,
            "transport": transport,
            "action": action,
            "text": text,
            "webContext": context,
            "pageUrl": context.page.as_ref().and_then(|page| page.url.clone()),
            "pageTitle": context.page.as_ref().and_then(|page| page.title.clone()),
        }),
    )
    .await?;
    if response.get("ok") == Some(&Value::Bool(true)) {
        return Ok(());
    }
    Err(helper_error(&response))
}

fn helper_script() -> Result<PathBuf, RobotError> {
    if let Some(path) = env::var_os("DEVICE_BOT_IOS_WEBVIEW_HELPER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(RobotError::CommandFailed(format!(
            "iOS WebView helper is missing at {}",
            path.display()
        )));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join("webview-helper/webview.mjs");
            if bundled.is_file() {
                return Ok(bundled);
            }
            let beside = dir.join("webview.mjs");
            if beside.is_file() {
                return Ok(beside);
            }
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("webview-helper/webview.mjs");
    if source.is_file() {
        return Ok(source);
    }
    Err(RobotError::CommandFailed(
        "iOS WebView helper is not installed. Native accessibility queries still work.".into(),
    ))
}

pub(crate) async fn exchange(script: &Path, request: &Value) -> Result<Value, RobotError> {
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            RobotError::CommandFailed(format!(
                "iOS WebView helper could not start node: {error}. Native accessibility queries still work."
            ))
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        RobotError::CommandFailed("iOS WebView helper stdin is unavailable".into())
    })?;
    let line = format!("{request}\n");
    stdin.write_all(line.as_bytes()).await.map_err(|error| {
        RobotError::CommandFailed(format!(
            "could not write the WebView helper request: {error}"
        ))
    })?;
    drop(stdin);
    let mut stdout = child.stdout.take().ok_or_else(|| {
        RobotError::CommandFailed("iOS WebView helper stdout is unavailable".into())
    })?;
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(25), stdout.read_to_end(&mut bytes))
        .await
        .map_err(|_| RobotError::CommandFailed("iOS WebView helper timed out".into()))?
        .map_err(|error| {
            RobotError::CommandFailed(format!("could not read the WebView helper: {error}"))
        })?;
    let _ = child.wait().await;
    let text = String::from_utf8_lossy(&bytes);
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| RobotError::CommandFailed("iOS WebView helper returned no result".into()))?;
    let value: Value = serde_json::from_str(line).map_err(|error| {
        RobotError::CommandFailed(format!("iOS WebView helper returned invalid JSON: {error}"))
    })?;
    if value.get("code").and_then(Value::as_str).is_some() {
        return Err(helper_error(&value));
    }
    Ok(value)
}

fn helper_error(value: &Value) -> RobotError {
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("inspect_unavailable");
    let message = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("iOS WebView inspect failed");
    match code {
        "helper_dependency_missing" => RobotError::CommandFailed(message.into()),
        "not_found" | "ambiguous" | "invalid" => RobotError::InvalidInput(message.into()),
        _ => RobotError::CommandFailed(message.into()),
    }
}

fn decode_elements(value: &Value) -> Result<Vec<UiElement>, RobotError> {
    let Some(elements) = value.get("elements").and_then(Value::as_array) else {
        return Err(helper_error(value));
    };
    elements
        .iter()
        .map(|element| {
            let bounds = &element["bounds"];
            Ok(UiElement {
                class_name: element["className"]
                    .as_str()
                    .unwrap_or("element")
                    .to_owned(),
                resource_id: element["resourceId"].as_str().map(str::to_owned),
                text: element["text"].as_str().map(str::to_owned),
                content_description: element["contentDescription"].as_str().map(str::to_owned),
                bounds: Bounds {
                    left: bounds["left"].as_i64().unwrap_or(0) as i32,
                    top: bounds["top"].as_i64().unwrap_or(0) as i32,
                    right: bounds["right"].as_i64().unwrap_or(0) as i32,
                    bottom: bounds["bottom"].as_i64().unwrap_or(0) as i32,
                },
                clickable: element["clickable"].as_bool().unwrap_or(false),
                enabled: element["enabled"].as_bool().unwrap_or(true),
                focused: element["focused"].as_bool(),
                checked: element["checked"].as_bool(),
                selected: element["selected"].as_bool(),
                value: element["value"].as_str().map(str::to_owned),
                element_type: element["elementType"]
                    .as_str()
                    .and_then(|kind| ElementType::parse(kind).ok()),
                web_context: element
                    .get("webContext")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| {
                        RobotError::CommandFailed(format!("invalid WebView context: {error}"))
                    })?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_context_uses_helper_output_and_reports_missing_frames() {
        let path = std::env::temp_dir().join(format!(
            "device-bot-webview-helper-{}.mjs",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"let body='';
process.stdin.on('data', (chunk) => { body += chunk; });
process.stdin.on('end', () => {
  const request = JSON.parse(body);
  const name = request.webContext?.framePath?.[0]?.name;
  if (name === 'missing') {
    process.stdout.write(JSON.stringify({code:'not_found', error:'The requested WebView frame was not found'}) + '\n');
    return;
  }
  process.stdout.write(JSON.stringify({elements:[{className:'button', text:'Pay', bounds:{left:1,top:2,right:11,bottom:12}, clickable:true, enabled:true, webContext:request.webContext}]}) + '\n');
});
"#,
        )
        .unwrap();
        let found = exchange(
            &path,
            &json!({"webContext": {"framePath": [{"name": "payment"}]}}),
        )
        .await
        .unwrap();
        let elements = decode_elements(&found).unwrap();
        assert_eq!(elements[0].text.as_deref(), Some("Pay"));
        assert_eq!(
            elements[0].web_context.as_ref().unwrap().frame_path[0]
                .name
                .as_deref(),
            Some("payment")
        );
        let missing = exchange(
            &path,
            &json!({"webContext": {"framePath": [{"name": "missing"}]}}),
        )
        .await
        .unwrap_err();
        assert!(missing.to_string().contains("not found"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn shipped_helper_reports_a_missing_debugger_dependency() {
        let node = Command::new("node").arg("--version").output().await;
        if node.is_err() {
            return;
        }
        // A clean extracted helper has no node_modules, independently of the
        // developer checkout's optional installed dependencies.
        let dir =
            std::env::temp_dir().join(format!("device-bot-helper-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("webview-helper");
        for name in [
            "webview.mjs",
            "dom-references.js",
            "dom-tap.js",
            "dom-scroll.js",
            "dom-edit.js",
        ] {
            std::fs::copy(source.join(name), dir.join(name)).unwrap();
        }
        let script = dir.join("webview.mjs");
        let error = exchange(
            &script,
            &json!({"bundleId": "example.app", "transport": "sim", "action": "list"}),
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        let _ = std::fs::remove_dir_all(dir);
        assert!(message.contains("appium-remote-debugger"), "{message}");
    }
}

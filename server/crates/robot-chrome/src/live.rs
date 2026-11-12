//! Browser-encoded WebRTC. No screenshots or frame payloads over MCP.
use super::*;
impl ChromeRobot {
    pub(super) async fn start_live(
        &self,
        id: &str,
        offer: &str,
    ) -> Result<Option<robot_core::NativeVideoStream>, RobotError> {
        let target = self.page(id).await?;
        let key = format!(
            "__device_bot_live_{}",
            REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let quoted = serde_json::to_string(&key).unwrap();
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
        // Activating the tab does not guarantee document focus while the user
        // operates QC. Keep this controlled page focused during capture setup;
        // Chrome's sharing permission prompt is still required.
        self.cdp(
            &target,
            "Emulation.setFocusEmulationEnabled",
            json!({"enabled":true}),
        )
        .await?;
        let script = include_str!("live.js")
            .replace("__KEY__", &quoted)
            .replace("__OFFER__", &serde_json::to_string(offer).unwrap());
        let (stop, mut stopped) = tokio::sync::watch::channel(false);
        let robot = self.clone();
        let cleanup_target = target.clone();
        let cleanup_key = quoted.clone();
        let done = tokio::spawn(async move {
            let mut seen = false;
            loop {
                tokio::select! { _=stopped.changed()=>break, _=tokio::time::sleep(Duration::from_secs(2))=>{} }
                let expression = format!(
                    "(() => {{const s=window[{cleanup_key}]; return s ? ((s.answer && ['closed','failed','disconnected'].includes(s.peer?.connectionState)) ? 'closed' : 'open') : 'gone';}})()"
                );
                match robot
                    .cdp(
                        &cleanup_target,
                        "Runtime.evaluate",
                        json!({"expression":expression,"returnByValue":true}),
                    )
                    .await
                {
                    Ok(value) => {
                        let status = value["result"]["value"]
                            .as_str()
                            .unwrap_or("gone")
                            .to_owned();
                        if status == "closed" || (status == "gone" && seen) {
                            break;
                        }
                        seen |= status == "open";
                    }
                    Err(_) => break,
                }
            }
            let expression = format!("window[{cleanup_key}]?.stop(); delete window[{cleanup_key}]");
            let _ = robot
                .cdp(
                    &cleanup_target,
                    "Runtime.evaluate",
                    json!({"expression":expression}),
                )
                .await;
            let _ = robot
                .cdp(
                    &cleanup_target,
                    "Emulation.setFocusEmulationEnabled",
                    json!({"enabled":false}),
                )
                .await;
        });
        let started = self
            .cdp(
                &target,
                "Runtime.evaluate",
                json!({"expression":script,"userGesture":true,"returnByValue":true}),
            )
            .await;
        if let Err(error) = started {
            drop(stop);
            let _ = done.await;
            return Err(error);
        }
        let result=tokio::time::timeout(Duration::from_secs(50),async {
            loop {
                let expression=format!("JSON.stringify({{answer:window[{quoted}]?.answer,error:window[{quoted}]?.error,errorName:window[{quoted}]?.errorName,phase:window[{quoted}]?.phase}})");
                let response=self.cdp(&target,"Runtime.evaluate",json!({"expression":expression,"returnByValue":true})).await?;
                let state:Value=serde_json::from_str(response["result"]["value"].as_str().unwrap_or("{}"))
                    .map_err(|e|RobotError::CommandFailed(e.to_string()))?;
                if capture_precondition_unavailable(&state) { return Ok(None); }
                if let Some(error)=state["error"].as_str(){return Err(RobotError::CommandFailed(error.into()));}
                if state["answer"].is_object(){return Ok(Some(state["answer"].clone()));}
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }).await;
        match result {
            Ok(Ok(Some(answer))) => Ok(Some(robot_core::NativeVideoStream {
                sdp: answer["sdp"].as_str().unwrap_or_default().into(),
                width: answer["width"].as_u64().unwrap_or(0) as u32,
                height: answer["height"].as_u64().unwrap_or(0) as u32,
                stop,
                done,
            })),
            Ok(Ok(None)) => {
                drop(stop);
                let _ = done.await;
                Ok(None)
            }
            other => {
                drop(stop);
                let _ = done.await;
                Err(match other {Ok(Err(e))=>e,_=>RobotError::CommandFailed("Web Live sharing timed out. Choose This Tab in Chrome and allow sharing, then reopen Live.".into())})
            }
        }
    }
}

// getDisplayMedia may reject an unfocused document or missing transient
// activation even on HTTPS. No capture was approved in that case. A permission
// denial (NotAllowedError) or a failure after capture began must remain an error.
fn capture_precondition_unavailable(state: &Value) -> bool {
    state["phase"] == "capture" && state["errorName"] == "InvalidStateError"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_capture_focus_uses_fallback_but_permission_denial_does_not() {
        assert!(capture_precondition_unavailable(&json!({
            "phase":"capture", "errorName":"InvalidStateError", "error":"InvalidStateError: Invalid state"
        })));
        assert!(!capture_precondition_unavailable(&json!({
            "phase":"capture", "errorName":"NotAllowedError", "error":"Permission denied"
        })));
        assert!(!capture_precondition_unavailable(&json!({
            "phase":"crop", "errorName":"InvalidStateError", "error":"Invalid crop target"
        })));
        assert!(!capture_precondition_unavailable(
            &json!({"phase":"capture"})
        ));
    }
}

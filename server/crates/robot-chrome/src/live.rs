//! Browser-encoded WebRTC. No screenshots or frame payloads over MCP.
use super::*;
impl ChromeRobot {
    pub(super) async fn start_live(
        &self,
        id: &str,
        offer: &str,
    ) -> Result<robot_core::NativeVideoStream, RobotError> {
        let target = self.page(id).await?;
        let key = format!(
            "__device_bot_live_{}",
            REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let quoted = serde_json::to_string(&key).unwrap();
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
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
                let expression=format!("JSON.stringify({{answer:window[{quoted}]?.answer,error:window[{quoted}]?.error}})");
                let response=self.cdp(&target,"Runtime.evaluate",json!({"expression":expression,"returnByValue":true})).await?;
                let state:Value=serde_json::from_str(response["result"]["value"].as_str().unwrap_or("{}"))
                    .map_err(|e|RobotError::CommandFailed(e.to_string()))?;
                if let Some(error)=state["error"].as_str(){return Err(RobotError::CommandFailed(error.into()));}
                if state["answer"].is_object(){return Ok(state["answer"].clone());}
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }).await;
        match result {
            Ok(Ok(answer)) => Ok(robot_core::NativeVideoStream {
                sdp: answer["sdp"].as_str().unwrap_or_default().into(),
                width: answer["width"].as_u64().unwrap_or(0) as u32,
                height: answer["height"].as_u64().unwrap_or(0) as u32,
                stop,
                done,
            }),
            other => {
                drop(stop);
                let _ = done.await;
                Err(match other {Ok(Err(e))=>e,_=>RobotError::CommandFailed("Web Live sharing timed out. Choose This Tab in Chrome and allow sharing, then reopen Live.".into())})
            }
        }
    }
}

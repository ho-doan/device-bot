//! One persistent browser connection owns one disposable incognito context.
use super::*;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub(super) struct BrowserConnection {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub poisoned: bool,
}

impl BrowserConnection {
    pub async fn connect(endpoint: &str) -> Result<Self, RobotError> {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(30), connect_async(endpoint))
            .await.map_err(|_| RobotError::CommandFailed(
                "Chrome connection approval timed out; allow remote debugging in Chrome and retry".into()))?
            .map_err(|e| RobotError::CommandFailed(format!("could not connect to running Chrome: {e}")))?;
        Ok(Self {
            socket,
            poisoned: false,
        })
    }

    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value, RobotError> {
        if self.poisoned {
            return Err(RobotError::CommandFailed("Chrome connection is closed after a previous transport failure; start a new MCP session".into()));
        }
        let id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            self.socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|e| {
                    RobotError::CommandFailed(format!(
                        "could not send Chrome command {method}: {e}"
                    ))
                })?;
            while let Some(frame) = self.socket.next().await {
                let frame = frame.map_err(|e| {
                    RobotError::CommandFailed(format!(
                        "Chrome transport failed during {method}: {e}"
                    ))
                })?;
                if frame.is_close() {
                    self.poisoned = true;
                    return Err(RobotError::CommandFailed(format!(
                        "Chrome closed the connection during {method}"
                    )));
                }
                if !frame.is_text() {
                    continue;
                }
                let response: Value = serde_json::from_str(
                    frame
                        .to_text()
                        .map_err(|e| RobotError::CommandFailed(e.to_string()))?,
                )
                .map_err(|e| RobotError::CommandFailed(format!("invalid Chrome JSON: {e}")))?;
                if response["id"].as_u64() != Some(id) {
                    continue;
                }
                if let Some(error) = response.get("error") {
                    return Err(RobotError::CommandFailed(format!(
                        "Chrome command {method} failed: {error}"
                    )));
                }
                return Ok(response.get("result").cloned().unwrap_or(Value::Null));
            }
            self.poisoned = true;
            Err(RobotError::CommandFailed(format!(
                "Chrome closed the connection during {method}"
            )))
        })
        .await;
        match result {
            Ok(result) => result,
            // Keep the shared connection alive. The next request skips any late
            // response by ID; a case timeout must not dispose QC's other contexts.
            Err(_) => Err(RobotError::CommandFailed(format!(
                "Chrome command {method} timed out after 5 seconds"
            ))),
        }
    }
}

pub(super) struct BrowserSession {
    connection: Arc<Mutex<BrowserConnection>>,
    pub context: String,
    sessions: HashMap<String, String>,
}

impl BrowserSession {
    pub async fn create(connection: Arc<Mutex<BrowserConnection>>) -> Result<Self, RobotError> {
        let created = connection
            .lock()
            .await
            .request(
                "Target.createBrowserContext",
                json!({"disposeOnDetach":true}),
                None,
            )
            .await?;
        let context = created["browserContextId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                RobotError::CommandFailed("Chrome did not return an incognito context ID".into())
            })?
            .to_owned();
        let mut session = Self {
            connection,
            context,
            sessions: HashMap::new(),
        };
        let context = session.context.clone();
        if let Err(error) = session
            .request(
                "Target.createTarget",
                json!({"url":"about:blank","newWindow":true,"browserContextId":context}),
                None,
            )
            .await
        {
            let _ = session.close().await;
            return Err(error);
        }
        Ok(session)
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value, RobotError> {
        self.connection
            .lock()
            .await
            .request(method, params, session)
            .await
    }

    pub async fn targets(&mut self) -> Result<Vec<ChromeTarget>, RobotError> {
        let response = self.request("Target.getTargets", json!({}), None).await?;
        let infos = response["targetInfos"].as_array().ok_or_else(|| {
            RobotError::CommandFailed("Chrome did not return target information".into())
        })?;
        infos
            .iter()
            .filter(|t| {
                t["browserContextId"].as_str() == Some(&self.context)
                    && t["type"] == "page"
                    && !t["url"].as_str().unwrap_or("").starts_with("devtools://")
            })
            .map(|info| {
                Ok(ChromeTarget {
                    id: info["targetId"]
                        .as_str()
                        .ok_or_else(|| RobotError::CommandFailed("Chrome target has no ID".into()))?
                        .into(),
                    title: info["title"].as_str().unwrap_or("").into(),
                    url: info["url"].as_str().unwrap_or("").into(),
                    kind: "page".into(),
                    // A scoped marker, never a personal page WebSocket endpoint.
                    web_socket_debugger_url: Some("scoped-context".into()),
                })
            })
            .collect()
    }

    pub async fn page_request(
        &mut self,
        target: &ChromeTarget,
        method: &str,
        params: Value,
    ) -> Result<Value, RobotError> {
        // Validate ownership before attaching; caller-supplied IDs cannot escape the context.
        if !self.targets().await?.iter().any(|t| t.id == target.id) {
            return Err(RobotError::DeviceNotFound(format!(
                "{} is outside this case context",
                target.id
            )));
        }
        let session = if let Some(session) = self.sessions.get(&target.id) {
            session.clone()
        } else {
            let attached = self
                .request(
                    "Target.attachToTarget",
                    json!({"targetId":target.id,"flatten":true}),
                    None,
                )
                .await?;
            let session = attached["sessionId"]
                .as_str()
                .ok_or_else(|| {
                    RobotError::CommandFailed("Chrome did not return a page session".into())
                })?
                .to_owned();
            self.sessions.insert(target.id.clone(), session.clone());
            session
        };
        self.request(method, params, Some(session)).await
    }

    pub async fn close(&mut self) -> Result<(), RobotError> {
        let context = self.context.clone();
        let result = self
            .request(
                "Target.disposeBrowserContext",
                json!({"browserContextId":context}),
                None,
            )
            .await;
        if result.is_ok() {
            self.context.clear();
        }
        result.map(|_| ())
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        if self.context.is_empty() {
            return;
        }
        let connection = Arc::clone(&self.connection);
        let context = std::mem::take(&mut self.context);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = connection
                    .lock()
                    .await
                    .request(
                        "Target.disposeBrowserContext",
                        json!({"browserContextId":context}),
                        None,
                    )
                    .await;
            });
        }
        // On process shutdown the browser connection closes and Chrome's
        // disposeOnDetach handles any remaining contexts from this backend.
    }
}

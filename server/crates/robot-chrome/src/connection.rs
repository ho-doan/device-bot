//! One persistent browser connection owns one disposable incognito context.
use super::*;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub(super) struct BrowserConnection {
    pub(super) socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub poisoned: bool,
    events: Vec<Value>,
    frame_attachments: HashMap<String, Value>,
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
            events: Vec::new(),
            frame_attachments: HashMap::new(),
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
                if response.get("id").is_none() {
                    self.record_event(response);
                    continue;
                }
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
            Ok(Err(error)) => {
                let message = error.to_string();
                if message.contains("transport failed")
                    || message.contains("could not send")
                    || message.contains("closed the connection")
                {
                    self.poisoned = true;
                }
                Err(error)
            }
            Ok(Ok(result)) => Ok(result),
            // Keep the shared connection alive. The next request skips any late
            // response by ID; a case timeout must not dispose QC's other contexts.
            Err(_) => Err(RobotError::CommandFailed(format!(
                "Chrome command {method} timed out after 5 seconds"
            ))),
        }
    }

    pub async fn collect_events(&mut self, budget: Duration) -> Result<Vec<Value>, RobotError> {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, self.socket.next()).await {
                Ok(Some(Ok(frame))) => {
                    if !frame.is_text() {
                        continue;
                    }
                    let event: Value = serde_json::from_str(
                        frame
                            .to_text()
                            .map_err(|error| RobotError::CommandFailed(error.to_string()))?,
                    )
                    .map_err(|error| {
                        RobotError::CommandFailed(format!("invalid Chrome JSON: {error}"))
                    })?;
                    if event.get("id").is_none() && event.get("method").is_some() {
                        self.record_event(event);
                    }
                }
                Ok(Some(Err(error))) => {
                    self.poisoned = true;
                    return Err(RobotError::CommandFailed(format!(
                        "Chrome transport failed while reading frame events: {error}"
                    )));
                }
                Ok(None) => {
                    self.poisoned = true;
                    return Err(RobotError::CommandFailed(
                        "Chrome closed the connection while reading frame events".into(),
                    ));
                }
                Err(_) => break,
            }
        }
        let mut events = std::mem::take(&mut self.events);
        events.retain(|event| event["method"] != "Target.attachedToTarget");
        events.extend(self.frame_attachments.values().cloned());
        Ok(events)
    }

    fn record_event(&mut self, event: Value) {
        if event["method"] == "Target.attachedToTarget" {
            if let Some(id) = event["params"]["sessionId"].as_str() {
                self.frame_attachments.insert(id.into(), event.clone());
            }
        } else if event["method"] == "Target.detachedFromTarget" {
            if let Some(id) = event["params"]["sessionId"].as_str() {
                let mut removed = std::collections::HashSet::from([id.to_owned()]);
                loop {
                    let before = removed.len();
                    for (child, attached) in &self.frame_attachments {
                        if attached["sessionId"]
                            .as_str()
                            .is_some_and(|parent| removed.contains(parent))
                        {
                            removed.insert(child.clone());
                        }
                    }
                    if before == removed.len() {
                        break;
                    }
                }
                self.frame_attachments.retain(|id, _| !removed.contains(id));
            }
        }
        if self.events.len() >= 1024 {
            self.events.remove(0);
        }
        self.events.push(event);
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

    pub async fn session_request(
        &mut self,
        session: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, RobotError> {
        self.request(method, params, Some(session.to_owned())).await
    }

    pub async fn collect_events(&mut self, budget: Duration) -> Result<Vec<Value>, RobotError> {
        self.connection.lock().await.collect_events(budget).await
    }

    /// Attaches descendant iframe targets and keeps only frames that belong to this page.
    pub async fn discover_frames(
        &mut self,
        target: &ChromeTarget,
    ) -> Result<
        (
            crate::frames::FrameNode,
            Vec<crate::frames::AuthorizedFrame>,
        ),
        RobotError,
    > {
        self.page_request(target, "Page.enable", json!({})).await?;
        self.page_request(target, "Runtime.enable", json!({}))
            .await?;
        self.page_request(
            target,
            "Target.setAutoAttach",
            json!({"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true}),
        )
        .await?;
        let events = self.collect_events(Duration::from_millis(200)).await?;
        let tree_value = self
            .page_request(target, "Page.getFrameTree", json!({}))
            .await?;
        let mut tree = crate::frames::parse_frame_tree(&tree_value)?;
        let mut owned = crate::frames::frame_ids(&tree);
        let page_session =
            self.sessions.get(&target.id).cloned().ok_or_else(|| {
                RobotError::CommandFailed("Chrome page session is missing".into())
            })?;
        let mut pending = Vec::new();
        for event in events {
            if event["method"] != "Target.attachedToTarget" {
                continue;
            }
            let target_type = event["params"]["targetInfo"]["type"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            let session_id = event["params"]["sessionId"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            if session_id.is_empty() || target_type != "iframe" {
                continue;
            }
            pending.push((session_id, event["sessionId"].as_str().map(str::to_owned)));
        }
        let mut accepted = Vec::new();
        let mut parents = std::collections::HashSet::from([page_session]);
        loop {
            let mut progress = false;
            let mut rest = Vec::new();
            for (session_id, parent_session_id) in pending.drain(..) {
                let parent_ok = parent_session_id
                    .as_deref()
                    .is_some_and(|parent| parents.contains(parent));
                if !parent_ok {
                    rest.push((session_id, parent_session_id));
                    continue;
                }
                let child_tree = self
                    .session_request(&session_id, "Page.getFrameTree", json!({}))
                    .await
                    .ok();
                let root_frame_id = child_tree
                    .as_ref()
                    .and_then(|value| value["frameTree"]["frame"]["id"].as_str())
                    .unwrap_or("")
                    .to_owned();
                let parsed_child = child_tree
                    .as_ref()
                    .and_then(|value| crate::frames::parse_frame_tree(value).ok());
                // CDP omits OOPIF roots from the parent's Page.getFrameTree.
                // Require both an authorized parent session and an explicit
                // parentFrameId in the owned tree before admitting that root.
                if owned.contains(&root_frame_id)
                    || parsed_child
                        .as_ref()
                        .and_then(|child| child.parent_id.as_ref())
                        .is_some_and(|parent| owned.contains(parent))
                {
                    if let Some(child_tree) = child_tree
                        .as_ref()
                        .and_then(|value| crate::frames::parse_frame_tree(value).ok())
                    {
                        crate::frames::merge_frame_tree(&mut tree, &child_tree);
                        owned = crate::frames::frame_ids(&tree);
                    }
                    self.session_request(
                        &session_id,
                        "Target.setAutoAttach",
                        json!({"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true}),
                    )
                    .await?;
                    parents.insert(session_id.clone());
                    accepted.push(crate::frames::AuthorizedFrame {
                        session_id,
                        frame_id: root_frame_id,
                    });
                    progress = true;
                } else {
                    let _ = self
                        .request(
                            "Target.detachFromTarget",
                            json!({"sessionId": session_id}),
                            None,
                        )
                        .await;
                }
            }
            pending = rest;
            if progress {
                let known: std::collections::HashSet<String> = accepted
                    .iter()
                    .map(|frame| frame.session_id.clone())
                    .chain(pending.iter().map(|(id, _)| id.clone()))
                    .collect();
                for event in self.collect_events(Duration::from_millis(50)).await? {
                    if event["method"] == "Target.attachedToTarget"
                        && event["params"]["targetInfo"]["type"] == "iframe"
                    {
                        if let Some(id) = event["params"]["sessionId"]
                            .as_str()
                            .filter(|id| !known.contains(*id))
                        {
                            pending
                                .push((id.into(), event["sessionId"].as_str().map(str::to_owned)));
                        }
                    }
                }
                if accepted.len() > 256 {
                    return Err(RobotError::InvalidInput(
                        "Chrome frame limit exceeded".into(),
                    ));
                }
            }
            if !progress {
                break;
            }
        }
        Ok((tree, accepted))
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

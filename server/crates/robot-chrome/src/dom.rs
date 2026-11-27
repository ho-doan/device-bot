//! Evaluates and normalizes DOM elements in the selected document.

use robot_core::{Bounds, ElementType, Point, RobotError, UiElement, WebContext};
use serde_json::{Value, json};

use super::{ChromeRobot, ChromeTarget, frames};

impl ChromeRobot {
    pub(super) async fn query_dom(
        &self,
        target: &ChromeTarget,
    ) -> Result<Vec<UiElement>, RobotError> {
        let expression = include_str!("dom-elements.js");
        let result = self
            .cdp(
                target,
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            let detail = exception
                .pointer("/exception/description")
                .or_else(|| exception.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("JavaScript evaluation failed");
            return Err(RobotError::CommandFailed(format!(
                "Chrome DOM/accessibility query failed: {detail}"
            )));
        }
        let values = result
            .pointer("/result/value")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                RobotError::CommandFailed("Chrome did not return DOM element data".into())
            })?;
        Ok(values
            .iter()
            .map(|value| UiElement {
                class_name: value_string(value, "className").unwrap_or_else(|| "element".into()),
                resource_id: value_string(value, "resourceId"),
                text: value_string(value, "text"),
                content_description: value_string(value, "contentDescription"),
                bounds: Bounds {
                    left: value_i32(value, "left"),
                    top: value_i32(value, "top"),
                    right: value_i32(value, "right"),
                    bottom: value_i32(value, "bottom"),
                },
                clickable: value
                    .get("clickable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                enabled: value
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                focused: value.get("focused").and_then(Value::as_bool),
                checked: value.get("checked").and_then(Value::as_bool),
                selected: value.get("selected").and_then(Value::as_bool),
                value: value
                    .get("value")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                element_type: value
                    .get("elementType")
                    .and_then(Value::as_str)
                    .and_then(|element_type| ElementType::parse(element_type).ok()),
                web_context: None,
            })
            .collect())
    }

    async fn session_eval(
        &self,
        target: &ChromeTarget,
        session: Option<&str>,
        frame_id: &str,
        expression: &str,
    ) -> Result<Value, RobotError> {
        let mut browser = self.browser_session.lock().await;
        let session_owner = browser.as_mut().ok_or_else(|| {
            RobotError::CommandFailed(
                "Chrome case context is not started; call mobile_boot_device with chrome:browser"
                    .into(),
            )
        })?;
        let created = match session {
            Some(session) => {
                session_owner
                    .session_request(
                        session,
                        "Page.createIsolatedWorld",
                        json!({"frameId": frame_id, "worldName": "device-bot"}),
                    )
                    .await?
            }
            None => {
                session_owner
                    .page_request(
                        target,
                        "Page.createIsolatedWorld",
                        json!({"frameId": frame_id, "worldName": "device-bot"}),
                    )
                    .await?
            }
        };
        let context_id = created["executionContextId"].as_u64().ok_or_else(|| {
            RobotError::CommandFailed("Chrome did not return a frame execution context".into())
        })?;
        let params = json!({
            "expression": expression,
            "contextId": context_id,
            "returnByValue": true,
            "awaitPromise": true,
        });
        match session {
            Some(session) => {
                session_owner
                    .session_request(session, "Runtime.evaluate", params)
                    .await
            }
            None => {
                session_owner
                    .page_request(target, "Runtime.evaluate", params)
                    .await
            }
        }
    }

    fn elements_from_value(
        value: &Value,
        context: Option<WebContext>,
        origin: Option<(i32, i32)>,
    ) -> Result<Vec<UiElement>, RobotError> {
        let values = value.as_array().ok_or_else(|| {
            RobotError::CommandFailed("Chrome did not return DOM element data".into())
        })?;
        Ok(values
            .iter()
            .map(|value| {
                let mut element = UiElement {
                    class_name: value_string(value, "className")
                        .unwrap_or_else(|| "element".into()),
                    resource_id: value_string(value, "resourceId"),
                    text: value_string(value, "text"),
                    content_description: value_string(value, "contentDescription"),
                    bounds: Bounds {
                        left: value_i32(value, "left"),
                        top: value_i32(value, "top"),
                        right: value_i32(value, "right"),
                        bottom: value_i32(value, "bottom"),
                    },
                    clickable: value
                        .get("clickable")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    enabled: value
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    focused: value.get("focused").and_then(Value::as_bool),
                    checked: value.get("checked").and_then(Value::as_bool),
                    selected: value.get("selected").and_then(Value::as_bool),
                    value: value
                        .get("value")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    element_type: value
                        .get("elementType")
                        .and_then(Value::as_str)
                        .and_then(|element_type| ElementType::parse(element_type).ok()),
                    web_context: context.clone().map(|mut context| {
                        context.element_ref = value_string(value, "elementRef");
                        context
                    }),
                };
                if let Some((x, y)) = origin {
                    element.bounds.left += x;
                    element.bounds.right += x;
                    element.bounds.top += y;
                    element.bounds.bottom += y;
                }
                element
            })
            .collect())
    }

    pub(super) async fn query_child_frames(
        &self,
        target: &ChromeTarget,
        include_root: bool,
    ) -> Result<Vec<UiElement>, RobotError> {
        let (tree, authorized) = {
            let mut browser = self.browser_session.lock().await;
            let session = browser.as_mut().ok_or_else(|| {
                RobotError::CommandFailed(
                    "Chrome case context is not started; call mobile_boot_device with chrome:browser"
                        .into(),
                )
            })?;
            session.discover_frames(target).await?
        };
        let owners_value = self
            .session_eval(target, None, &tree.id, include_str!("frame-owners.js"))
            .await?;
        self.ensure_eval_ok(&owners_value)?;
        let raw_owners = owners_value
            .pointer("/result/value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut owners = frames::bind_owners(&tree.children, &raw_owners);
        for child in frames::descendant_frames(&tree) {
            let child_session = frames::session_for_frame(&tree, &child.id, &authorized);
            if let Ok(nested) = self
                .session_eval(
                    target,
                    child_session,
                    &child.id,
                    include_str!("frame-owners.js"),
                )
                .await
            {
                if let Some(values) = nested.pointer("/result/value").and_then(Value::as_array) {
                    if let Some(node) = frames::find_frame(&tree, &child.id) {
                        owners.extend(frames::bind_owners(&node.children, values));
                    }
                }
            }
        }
        let expression = format!(
            "(async () => {{ const deviceBotReferences = {}; const elements = await ({}); return {{elements, truncated: elements.length >= 500}}; }})()",
            include_str!("../../robot-ios-real/webview-helper/dom-references.js"),
            include_str!("dom-elements.js")
        );
        let mut collected = Vec::new();
        let mut documents = frames::descendant_frames(&tree);
        if include_root {
            documents.insert(0, &tree);
        }
        for frame in documents {
            let session = frames::session_for_frame(&tree, &frame.id, &authorized);
            let evaluated = self
                .session_eval(target, session, &frame.id, &expression)
                .await?;
            self.ensure_eval_ok(&evaluated)?;
            let payload = evaluated
                .pointer("/result/value")
                .cloned()
                .unwrap_or(Value::Null);
            let context = frames::context_for_frame(&tree, &frame.id, &owners)?;
            let chain = frames::frame_chain(&tree, &frame.id);
            let origin = frames::map_bounds(
                Bounds {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 1,
                },
                &owners,
                &chain,
            )?;
            let elements = payload["elements"].as_array().cloned().unwrap_or_default();
            collected.extend(Self::elements_from_value(
                &Value::Array(elements),
                Some(context),
                Some((origin.left, origin.top)),
            )?);
        }
        Ok(collected)
    }

    fn ensure_eval_ok(&self, result: &Value) -> Result<(), RobotError> {
        if let Some(exception) = result.get("exceptionDetails") {
            let detail = exception
                .pointer("/exception/description")
                .or_else(|| exception.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("JavaScript evaluation failed");
            return Err(RobotError::CommandFailed(format!(
                "Chrome DOM/accessibility query failed: {detail}"
            )));
        }
        Ok(())
    }

    pub(super) async fn eval_in_context(
        &self,
        target: &ChromeTarget,
        context: &WebContext,
        expression: &str,
    ) -> Result<(), RobotError> {
        self.eval_in_context_value(target, context, expression, false)
            .await
            .map(|_| ())
    }

    pub(super) async fn eval_in_context_value(
        &self,
        target: &ChromeTarget,
        context: &WebContext,
        expression: &str,
        map_pointer: bool,
    ) -> Result<(Value, Vec<Point>), RobotError> {
        if let Some(page) = &context.page {
            if page.url.as_deref().is_some_and(|url| url != target.url)
                || page
                    .title
                    .as_deref()
                    .is_some_and(|title| title != target.title)
            {
                return Err(RobotError::InvalidInput(
                    "The selected Chrome page changed; inspect it again".into(),
                ));
            }
        }
        let (tree, authorized) = {
            let mut browser = self.browser_session.lock().await;
            let session = browser.as_mut().ok_or_else(|| {
                RobotError::CommandFailed("Chrome case context is not started".into())
            })?;
            session.discover_frames(target).await?
        };
        let owners_value = self
            .session_eval(target, None, &tree.id, include_str!("frame-owners.js"))
            .await?;
        self.ensure_eval_ok(&owners_value)?;
        let raw_owners = owners_value
            .pointer("/result/value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut owners = frames::bind_owners(&tree.children, &raw_owners);
        for child in frames::descendant_frames(&tree) {
            let child_session = frames::session_for_frame(&tree, &child.id, &authorized);
            if let Ok(nested) = self
                .session_eval(
                    target,
                    child_session,
                    &child.id,
                    include_str!("frame-owners.js"),
                )
                .await
            {
                if let Some(values) = nested.pointer("/result/value").and_then(Value::as_array) {
                    if let Some(node) = frames::find_frame(&tree, &child.id) {
                        owners.extend(frames::bind_owners(&node.children, values));
                    }
                }
            }
        }
        let frame = frames::resolve_frame(&tree, &owners, context)?;
        let session = frames::session_for_frame(&tree, &frame.id, &authorized);
        let result = self
            .session_eval(target, session, &frame.id, expression)
            .await?;
        self.ensure_eval_ok(&result)?;
        let mapping = if map_pointer {
            let chain = frames::frame_chain(&tree, &frame.id);
            let origin = frames::map_bounds(
                Bounds {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                &owners,
                &chain,
            )?;
            let coordinate = |point: &Value, axis: &str, offset: i32| {
                point[axis]
                    .as_f64()
                    .map(|value| value + f64::from(offset))
                    .filter(|value| value.is_finite() && *value >= 0.)
                    .ok_or_else(|| {
                        RobotError::InvalidInput("DOM element has no visible pointer point".into())
                    })
            };
            let points = result["result"]["value"]
                .as_array()
                .ok_or_else(|| RobotError::InvalidInput("Missing DOM pointer mapping".into()))?
                .iter()
                .map(|value| {
                    Ok(Point {
                        x: coordinate(value, "x", origin.left)?,
                        y: coordinate(value, "y", origin.top)?,
                    })
                })
                .collect::<Result<Vec<_>, RobotError>>()?;
            let mut parent = &tree;
            let mut parent_points: Vec<_> = points.iter().map(|p| [p.x, p.y]).collect();
            for child in chain {
                let owner = owners
                    .iter()
                    .find(|owner| owner.frame_id == child.id)
                    .ok_or_else(|| RobotError::InvalidInput("Missing DOM frame owner".into()))?;
                // Root CDP node lookup is not enough for out-of-process iframes.
                // Verify the expected hit in each parent document before dispatch.
                let expression = format!(
                    r#"(() => {{
                      const e = document.querySelectorAll('iframe,frame')[{}];
                      return !!e && (e.name || '') === {} && (e.src || '') === {}
                        && {}.every(([x, y]) => document.elementFromPoint(x, y) === e);
                    }})()"#,
                    owner.dom_index,
                    json!(owner.name),
                    json!(owner.url),
                    json!(parent_points),
                );
                let parent_session = frames::session_for_frame(&tree, &parent.id, &authorized);
                let hit = self
                    .session_eval(target, parent_session, &parent.id, &expression)
                    .await?;
                self.ensure_eval_ok(&hit)?;
                if hit["result"]["value"] != true {
                    return Err(RobotError::InvalidInput(
                        "Chrome frame is covered or its pointer coordinates changed; inspect it again".into(),
                    ));
                }
                parent = child;
                for point in &mut parent_points {
                    point[0] -= owner.left;
                    point[1] -= owner.top;
                }
            }
            points
        } else {
            Vec::new()
        };
        Ok((result, mapping))
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn value_i32(value: &Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_default()
        .round() as i32
}

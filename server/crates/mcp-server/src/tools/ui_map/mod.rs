//! MCP entry point, job lifecycle, and artifact persistence for UI maps.
mod crawl;
mod model;
mod replay;
mod timing;
mod viewport;

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use device_mcp_protocol::{ToolDefinition, ToolResult};
use robot_core::Robot;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crawl::Options;
use model::{Control, Screen, SourceRoute, Transition, UiMap};

pub(super) const NAME: &str = "mobile_ui_map";
const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
static ACTIVE: LazyLock<Mutex<HashMap<String, LeaseInfo>>> = LazyLock::new(Mutex::default);
static SOURCE_UPDATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct LeaseInfo {
    id: String,
    stop: Arc<AtomicBool>,
}

struct Lease {
    key: String,
}

impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE.lock().unwrap().remove(&self.key);
    }
}

fn acquire(device: &str, id: &str) -> Result<(Lease, Arc<AtomicBool>), String> {
    let key = device
        .split(':')
        .next()
        .filter(|p| matches!(*p, "macos" | "windows"))
        .unwrap_or(device)
        .to_owned();
    let mut active = ACTIVE.lock().unwrap();
    if active.contains_key(&key) {
        return Err("A UI map scan is already running on this target.".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    active.insert(
        key.clone(),
        LeaseInfo {
            id: id.into(),
            stop: stop.clone(),
        },
    );
    Ok((Lease { key }, stop))
}

#[derive(Default)]
pub(super) struct Jobs {
    tasks: tokio::sync::Mutex<Vec<(Arc<AtomicBool>, JoinHandle<()>)>>,
}

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        title: Some("Explore UI screens and find navigation routes".into()),
        description: "Create a bounded UI map from the app or Chrome tab already open. Start returns mapId; poll status, list saved maps, request a screenshot, find a route, or retrieve action evidence with its generated YAML. Pass exact allowedTaps, synthetic inputs, or allButtons for an isolated target. Tap selection does not exclude controls by name. Web stays on the initial origin. add_source_routes stores source findings separately from observed routes.".into(),
        input_schema: json!({
            "type":"object",
            "properties":{
                "action":{"enum":["start","status","stop","list","route","screenshot","action_evidence","add_source_routes"]},
                "deviceId":{"type":"string","minLength":1,"maxLength":270},
                "mapId":{"type":"string","format":"uuid"},
                "screenId":{"type":"string","pattern":"^s[0-9]+$"},
                "controlId":{"type":"string","pattern":"^c[0-9]+$"},
                "from":{"type":"string","pattern":"^s[0-9]+$"},
                "to":{"type":"string","description":"Screen ID, exact screen title, or exact source route"},
                "allButtons":{"type":"boolean"},
                "allowedTaps":{"type":"array","maxItems":150,"items":{"type":"string"}},
                "fixtureControls":{"type":"array","maxItems":30,"items":{"type":"string"},"description":"Legacy additional exact tap selectors; new callers can use allowedTaps."},
                "expandSelectionStates":{"type":"boolean","description":"Explore every checked or selected variant even if it exposes the same actionable controls"},
                "inputs":{"type":"object","maxProperties":30,"additionalProperties":{"type":"string"}},
                "backControl":{"type":"string"},
                "resetAppId":{"type":"string","description":"Opt in to relaunching the foreground mobile fixture or selected single-window macOS fixture. Must match its app or bundle ID."},
                "maxScreens":{"type":"integer","minimum":1,"maximum":crawl::MAX_SCREENS},
                "maxActions":{"type":"integer","minimum":1,"maximum":crawl::MAX_ACTIONS},
                "maxDepth":{"type":"integer","minimum":1,"maximum":crawl::MAX_DEPTH},
                "settleMs":{"type":"integer","minimum":100,"maximum":2500},
                "maxTimeSeconds":{"type":"integer","minimum":30,"maximum":crawl::MAX_TIME_SECONDS,"description":"Optional elapsed-time limit. Omit to scan without a total time limit."},
                "sourceRoutes":{"type":"array","maxItems":100,"items":{
                    "type":"object","required":["title","route","sourcePath","line","status"],
                    "properties":{"title":{"type":"string"},"route":{"type":"string"},
                        "sourcePath":{"type":"string"},"line":{"type":"integer","minimum":1},
                        "status":{"enum":["candidate","entry_unknown","unsupported"]},
                        "steps":{"type":"array","items":{"type":"string"}}},
                    "additionalProperties":false
                }}
            },
            "required":["action"],
            "additionalProperties":false
        }),
    }
}

impl Jobs {
    pub(super) async fn call(&self, robot: &Arc<dyn Robot>, args: &Value) -> ToolResult {
        match self.operate(robot, args).await {
            Ok(value) => ToolResult::structured(value),
            Err(error) => ToolResult::execution_error(error),
        }
    }

    async fn operate(&self, robot: &Arc<dyn Robot>, args: &Value) -> Result<Value, String> {
        let root = robot_report::store::default_reports_dir()
            .map_err(|e| e.to_string())?
            .join("ui-maps");
        let action = required(args, "action")?;
        if action == "start" {
            let mut raw = args.clone();
            raw.as_object_mut()
                .ok_or("Expected scan arguments")?
                .remove("action");
            let options: Options = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            return self.start(robot.clone(), &root, options).await;
        }
        if action == "list" {
            return list(&root, args["deviceId"].as_str()).await;
        }
        let map_id = required(args, "mapId")?;
        let dir = map_dir(&root, map_id)?;
        if action == "stop" {
            if let Some(job) = ACTIVE.lock().unwrap().values().find(|job| job.id == map_id) {
                job.stop.store(true, Ordering::Release);
                return Ok(json!({"mapId":map_id,"status":"stopping"}));
            }
            return Ok(json!({"mapId":map_id,"status":read_map(&dir).await?.status}));
        }
        if action == "add_source_routes" {
            let _lock = SOURCE_UPDATE.lock().await;
            let mut map = read_map(&dir).await?;
            if map.status == "running" {
                return Err("Wait for the scan to stop before adding source routes.".into());
            }
            let routes: Vec<SourceRoute> =
                serde_json::from_value(args["sourceRoutes"].clone()).map_err(|e| e.to_string())?;
            if routes.is_empty() || routes.len() > 100 || !routes.iter().all(SourceRoute::valid) {
                return Err("Supply 1..100 valid source routes with repository-relative paths and line numbers.".into());
            }
            for route in routes {
                map.source_routes.retain(|other| {
                    other.route != route.route
                        || other.source_path != route.source_path
                        || other.line != route.line
                });
                map.source_routes.push(route);
            }
            if map.source_routes.len() > 400 {
                return Err("A map may hold at most 400 draft source routes.".into());
            }
            save_map(&dir, &map).await?;
            return Ok(json!({"mapId":map_id,"sourceRouteCount":map.source_routes.len()}));
        }
        let map = read_map(&dir).await?;
        match action {
            "status" => serde_json::to_value(map).map_err(|e| e.to_string()),
            "route" => {
                let from = args["from"]
                    .as_str()
                    .or_else(|| map.screens.first().map(|s| s.id.as_str()))
                    .ok_or("Map does not have an entry screen yet.")?;
                let to = required(args, "to")?;
                route_result(&map, from, to)
            }
            "screenshot" => {
                let id = required(args, "screenId")?;
                if !map
                    .screens
                    .iter()
                    .any(|node| node.screenshot && node.id == id)
                    || !is_screen_id(id)
                {
                    return Err("No screenshot was captured for this screen.".into());
                }
                let png = read_image(&dir.join(format!("{id}.png"))).await?;
                Ok(json!({"screenId":id,"mimeType":"image/png","base64":png}))
            }
            "action_evidence" => {
                let screen_id = required(args, "screenId")?;
                let control_id = required(args, "controlId")?;
                let evidence = map
                    .screens
                    .iter()
                    .find(|screen| screen.id == screen_id)
                    .and_then(|screen| {
                        screen
                            .controls
                            .iter()
                            .find(|control| control.id == control_id)
                    })
                    .and_then(|control| control.evidence.as_ref())
                    .ok_or("No action evidence was captured for this control.")?;
                if !is_screen_id(screen_id)
                    || !control_id.strip_prefix('c').is_some_and(|id| {
                        !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
                    })
                {
                    return Err("Invalid screen or control ID.".into());
                }
                let action_dir = dir
                    .join("actions")
                    .join(format!("{screen_id}-{control_id}"));
                let yaml = tokio::fs::read_to_string(action_dir.join("scenario.yaml"))
                    .await
                    .map_err(|_| "UI map action YAML is missing.")?;
                let screenshot = if evidence.screenshot.is_some() {
                    Some(read_image(&action_dir.join("result.png")).await?)
                } else {
                    None
                };
                Ok(
                    json!({"screenId":screen_id,"controlId":control_id,"yaml":yaml,"mimeType":"image/png","base64":screenshot}),
                )
            }
            _ => Err("Unknown UI map action.".into()),
        }
    }

    async fn start(
        &self,
        robot: Arc<dyn Robot>,
        root: &Path,
        options: Options,
    ) -> Result<Value, String> {
        options.validate()?;
        let id = Uuid::new_v4().to_string();
        let (lease, stop) = acquire(&options.device_id, &id)?;
        let dir = root.join(&id);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| e.to_string())?;
        let mut map = UiMap {
            id: id.clone(), device_id: options.device_id.clone(), status: "running".into(),
            started_at_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            attempted: 0, screens: Vec::new(), transitions: Vec::new(), source_routes: Vec::new(),
            notes: vec!["Coverage is limited to exposed controls and observable screen signatures. Draft source routes are not runtime evidence.".into()],
        };
        save_map(&dir, &map).await?;
        let cancel = stop.clone();
        let task = tokio::spawn(async move {
            let _lease = lease;
            let timing = timing::TimingLog::new();
            let scan = crawl::scan(&robot, &options, &cancel, &mut map, &dir, &timing);
            let outcome = if let Some(seconds) = options.max_time_seconds {
                tokio::time::timeout(Duration::from_secs(seconds + 30), scan)
                    .await
                    .unwrap_or_else(|_| Err("Scan time limit reached.".into()))
            } else {
                scan.await
            };
            match outcome {
                Ok(()) => {
                    let untested = map.screens.iter().flat_map(|s| &s.controls).any(|c| {
                        matches!(c.status.as_str(), "pending" | "failed")
                            || c.status == "skipped" && c.note.as_deref() != Some("Disabled")
                    });
                    map.status = if untested || map.notes.len() > 1 {
                        "partial"
                    } else {
                        "completed"
                    }
                    .into();
                }
                Err(e) => {
                    map.status = if cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else if map.screens.is_empty() {
                        "failed"
                    } else {
                        "partial"
                    }
                    .into();
                    map.notes.push(e);
                }
            }
            if let Err(error) = timing.save(&dir).await {
                eprintln!("Could not save UI map timings: {error}");
            }
            if let Err(error) = save_map(&dir, &map).await {
                eprintln!("Could not save UI map progress: {error}");
            }
        });
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|(_, task)| !task.is_finished());
        tasks.push((stop, task));
        Ok(json!({"mapId":id,"status":"running"}))
    }

    pub(super) async fn shutdown(&self) {
        let tasks = std::mem::take(&mut *self.tasks.lock().await);
        for (stop, _) in &tasks {
            stop.store(true, Ordering::Release);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        for (_, mut task) in tasks {
            if tokio::time::timeout_at(deadline, &mut task).await.is_err() {
                task.abort();
            }
        }
    }
}

async fn read_image(path: &Path) -> Result<String, String> {
    if tokio::fs::metadata(path)
        .await
        .map_err(|_| "UI map screenshot is missing.")?
        .len()
        > MAX_IMAGE_BYTES as u64
    {
        return Err("UI map screenshot exceeds the size limit.".into());
    }
    let png = tokio::fs::read(path)
        .await
        .map_err(|_| "UI map screenshot is missing.")?;
    Ok(STANDARD.encode(png))
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{key} is required."))
}

fn map_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    let canonical = Uuid::parse_str(id)
        .map_err(|_| "Invalid UI map ID.")?
        .to_string();
    if canonical != id {
        return Err("Invalid UI map ID.".into());
    }
    Ok(root.join(canonical))
}

fn is_screen_id(id: &str) -> bool {
    id.strip_prefix('s')
        .is_some_and(|number| number.parse::<usize>().is_ok())
}

async fn save_map(dir: &Path, map: &UiMap) -> Result<(), String> {
    let contents = serde_json::to_vec(map).map_err(|e| e.to_string())?;
    let temp = dir.join("map.json.tmp");
    tokio::fs::write(&temp, contents)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temp, dir.join("map.json"))
        .await
        .map_err(|e| e.to_string())
}

async fn read_map(dir: &Path) -> Result<UiMap, String> {
    let contents = tokio::fs::read(dir.join("map.json"))
        .await
        .map_err(|_| "UI map not found.")?;
    let mut map: UiMap = serde_json::from_slice(&contents).map_err(|e| e.to_string())?;
    if map.status == "running" && !ACTIVE.lock().unwrap().values().any(|job| job.id == map.id) {
        map.status = "partial".into();
        map.notes
            .push("Previous server stopped before this scan completed.".into());
    }
    Ok(map)
}

async fn list(root: &Path, device: Option<&str>) -> Result<Value, String> {
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"maps":[]})),
        Err(e) => return Err(e.to_string()),
    };
    let mut items = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        if let Ok(map) = read_map(&entry.path()).await
            && device.is_none_or(|expected| expected == map.device_id)
        {
            items.push(
                json!({"mapId":map.id,"deviceId":map.device_id,"status":map.status,
                "startedAtMs":map.started_at_ms,"screenCount":map.screens.len(),
                "title":map.screens.first().map(|s| s.title.as_str())}),
            );
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item["startedAtMs"].as_u64()));
    items.truncate(60);
    Ok(json!({"maps":items}))
}

fn route_result(map: &UiMap, from: &str, to: &str) -> Result<Value, String> {
    if !map.screens.iter().any(|screen| screen.id == from) {
        return Err("Unknown starting screen ID.".into());
    }
    let exact = map.screens.iter().find(|screen| screen.id == to);
    let matches: Vec<_> = map
        .screens
        .iter()
        .filter(|screen| {
            screen.title.eq_ignore_ascii_case(to) || screen.location.as_deref() == Some(to)
        })
        .collect();
    let target = match (exact, matches.as_slice()) {
        (Some(screen), _) => Some(screen),
        (None, [screen]) => Some(*screen),
        (None, [_, _, ..]) => {
            return Err("Screen name is ambiguous. Use a screen ID from status.".into());
        }
        _ => None,
    };
    if let Some(screen) = target {
        return Ok(match map.route(from, &screen.id) {
            Some(steps) => json!({"status":"observed","from":from,"to":screen.id,"steps":steps}),
            None => json!({"status":"no_observed_route","from":from,"to":screen.id,"steps":[]}),
        });
    }
    let candidates: Vec<_> = map
        .source_routes
        .iter()
        .filter(|candidate| candidate.route == to || candidate.title.eq_ignore_ascii_case(to))
        .collect();
    Ok(if candidates.is_empty() {
        json!({"status":"no_observed_route","from":from,"to":to,"steps":[]})
    } else {
        json!({"status":"source_only","from":from,"to":to,"steps":[],"sourceRoutes":candidates})
    })
}

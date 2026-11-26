//! Persisted UI observations. Draft source routes never become observed edges.
use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMap {
    pub id: String,
    pub device_id: String,
    pub status: String,
    pub started_at_ms: u64,
    pub attempted: usize,
    pub screens: Vec<Screen>,
    pub transitions: Vec<Transition>,
    pub source_routes: Vec<SourceRoute>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub id: String,
    pub title: String,
    pub location: Option<String>,
    pub screenshot: bool,
    pub controls: Vec<Control>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: String,
    pub label: String,
    pub resource_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub control: String,
    pub label: String,
    pub resource_id: Option<String>,
    pub action: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRoute {
    pub title: String,
    pub route: String,
    pub source_path: String,
    pub line: u32,
    pub status: SourceStatus,
    #[serde(default)]
    pub steps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Candidate,
    EntryUnknown,
    Unsupported,
}

impl SourceRoute {
    pub fn valid(&self) -> bool {
        fn bounded(s: &str, max: usize) -> bool {
            !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
        }
        bounded(&self.title, 160)
            && bounded(&self.route, 400)
            && bounded(&self.source_path, 400)
            && !self.source_path.starts_with('/')
            && !self.source_path.contains(['\\', ':'])
            && self
                .source_path
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."))
            && self.line > 0
            && self.steps.len() <= 30
            && self.steps.iter().all(|s| bounded(s, 300))
    }
}

impl UiMap {
    /// BFS walks only observed transitions. A candidate sourced from code is
    /// intentionally not a path until the corresponding UI was exercised.
    pub fn route(&self, from: &str, to: &str) -> Option<Vec<&Transition>> {
        if !self.screens.iter().any(|node| node.id == from)
            || !self.screens.iter().any(|node| node.id == to)
        {
            return None;
        }
        let mut previous = HashMap::<&str, &Transition>::new();
        let mut seen = HashSet::from([from]);
        let mut queue = VecDeque::from([from]);
        while let Some(node) = queue.pop_front() {
            if node == to {
                let mut chain = Vec::new();
                let mut cursor = node;
                while let Some(edge) = previous.get(cursor) {
                    chain.push(*edge);
                    cursor = edge.from.as_str();
                }
                chain.reverse();
                return Some(chain);
            }
            for edge in self.transitions.iter().filter(|edge| edge.from == node) {
                if seen.insert(edge.to.as_str()) {
                    previous.insert(edge.to.as_str(), edge);
                    queue.push_back(edge.to.as_str());
                }
            }
        }
        None
    }
}

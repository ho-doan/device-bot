use std::{collections::BTreeMap, future::Future, path::Path, sync::Mutex};

use serde::Serialize;
use tokio::time::Instant;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Phase {
    calls: u64,
    total_ms: u64,
    max_ms: u64,
}

pub(super) struct TimingLog {
    start: Instant,
    phases: Mutex<BTreeMap<&'static str, Phase>>,
}

impl TimingLog {
    pub(super) fn new() -> Self {
        Self {
            start: Instant::now(),
            phases: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) async fn measure<T>(
        &self,
        name: &'static str,
        future: impl Future<Output = T>,
    ) -> T {
        let _timer = Timer {
            log: self,
            name,
            start: Instant::now(),
        };
        future.await
    }

    pub(super) async fn save(&self, dir: &Path) -> Result<(), String> {
        let elapsed = self.start.elapsed().as_millis() as u64;
        let json = {
            let phases = self.phases.lock().map_err(|e| e.to_string())?;
            serde_json::to_vec_pretty(&serde_json::json!({
                "elapsedMs": elapsed,
                "phases": &*phases,
            }))
            .map_err(|e| e.to_string())?
        };
        let temporary = dir.join(".timing.json.tmp");
        tokio::fs::write(&temporary, json)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(temporary, dir.join("timing.json"))
            .await
            .map_err(|e| e.to_string())
    }
}

struct Timer<'a> {
    log: &'a TimingLog,
    name: &'static str,
    start: Instant,
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_millis() as u64;
        if let Ok(mut phases) = self.log.phases.lock() {
            let phase = phases.entry(self.name).or_default();
            phase.calls += 1;
            phase.total_ms += elapsed;
            phase.max_ms = phase.max_ms.max(elapsed);
        }
    }
}

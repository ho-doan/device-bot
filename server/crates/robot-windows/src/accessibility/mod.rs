//! Native Windows accessibility providers.

use crate::{MAX_UI_DEPTH, MAX_UI_NODES};

pub(super) mod msaa;

pub(super) fn build_uia_script(process_id: Option<u32>, max_results: Option<usize>) -> String {
    let mut script = include_str!("uia.ps1").to_owned();
    script = script.replace("__PROCESS_ID__", &process_id.unwrap_or(0).to_string());
    script = script.replace(
        "__MAX_RESULTS__",
        &max_results
            .unwrap_or(MAX_UI_NODES)
            .min(MAX_UI_NODES)
            .to_string(),
    );
    script = script.replace("__MAX_NODES__", &MAX_UI_NODES.to_string());
    script.replace("__MAX_DEPTH__", &MAX_UI_DEPTH.to_string())
}

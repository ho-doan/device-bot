# UI map in QC and MCP

Open the target app or Chrome tab first. In QC, the UI map icon beside the
target's Live and Screenshot buttons opens the reusable graph. Both the QC
desktop client and QC Web use the same graph widget and MCP job.
Use **UI maps** in the QC toolbar to reopen saved graphs even when the target
is disconnected.

Start with **Start scan**. With no actions selected the scanner inventories the
current screen only. Supply exact visible labels or resource IDs for taps, or
enable **Try all eligible buttons** for an isolated test target. The latter
is not a side-effect sandbox. The crawler skips known sensitive controls,
unlabeled or ambiguous targets, disabled controls, and input fields without
configured synthetic values. Use a test session and avoid secrets in fixtures
or captured screens. Values used to fill editors are not stored in the graph.
A screenshot can still show displayed test data.

Nodes are observed screen signatures. A directed edge records the control
and action that led from one state to another. Select a node to inspect its
screenshot, attempted and skipped controls, and shortest observed path from
entry. Use the search list when the graph gets dense. **Stop** ends a scan
after its current operation and retains the partial map. Reopen saved maps
from QC or MCP. Artifacts are persisted in `ui-maps/` under Device Bot's
configured report data directory.

## Exploration behavior and limits

The crawler uses the selected device's accessibility tree and real input
driver. It visits newly observed states, deduplicates signatures, verifies
the route back to each branch before continuing, and bounds depth, screens,
actions and runtime. Web restarts from the initial URL when it needs to
replay a branch and stops on an origin change. Android stops if the foreground
app changes. Native navigation can use one unique accessible Back, Close,
or Cancel control, or an exact configured `backControl`. If a native screen
cannot be restored, the scan reports partial coverage rather than sending
input to an unverified screen. Desktop maps use an explicitly selected
process/window, just like desktop Live.
The default scan budget is 24 screens, 45 actions, 5 levels and 8 minutes.
Use **Scan limits** to change it for larger test targets.

This is an inventory of reachable and observable UI states, not proof of
exhaustive app coverage or a test verdict. Off-screen items, permissions,
external origins, controls hidden from accessibility, conditional data and
states not exposed without a fixture may need a separate scan or flow.
Fingerprinting ignores field values, focus and geometry that may vary without
a navigation change. Similar-looking screens can be grouped if no distinguishing
accessible heading, location or control is exposed.

## MCP operations

The `mobile_ui_map` tool uses a single action argument. These are example
argument objects for `tools/call`:

```json
{"action":"start","deviceId":"chrome:<tab-id>","allowedTaps":["open-inbox","Open Settings"],"inputs":{"search-box":"sample search"},"maxScreens":30,"maxActions":60}
{"action":"status","mapId":"<map-id>"}
{"action":"route","mapId":"<map-id>","from":"s1","to":"s4"}
{"action":"screenshot","mapId":"<map-id>","screenId":"s4"}
{"action":"stop","mapId":"<map-id>"}
{"action":"list"}
```

Status returns screen IDs, controls, observations, source-only routes and
coverage notes. Copy an observed screen ID to query a shortest path. An exact
screen title or a saved source-route name/path also works. Always distinguish
`observed`, `no_observed_route`, and `source_only`.

## Give an agent the target source when it exists

Share the target checkout or relevant route definitions and navigation
handlers with the agent through its normal workspace access. No app code
needs to reside in Device Bot and the MCP service does not open paths in a
customer repository. The agent should:

1. Read route declarations, caller handlers and platform guards. For each
   target, record a repository-relative file/line, supported platforms, and
   visible actions from an entry state.
2. Start a UI map with known labels/IDs and synthetic fixture inputs. Check
   each discovered path against runtime screenshots and controls. If a
   route requires credentials or app-specific prerequisites, supply them
   through the test environment, not as source-map metadata.
3. Attach remaining source findings as separate records. Use `candidate`
   for a plausible path, `entry_unknown` for a declaration with no known
   caller, and `unsupported` only when source evidence shows the target
   is unavailable for the checked platform. Do not count them as observed.

```json
{
  "action": "add_source_routes",
  "mapId": "<map-id>",
  "sourceRoutes": [{
    "title": "Settings",
    "route": "/settings",
    "sourcePath": "lib/navigation/router.dart",
    "line": 42,
    "status": "candidate",
    "steps": ["Open the account menu", "Choose Settings"]
  }]
}
```

The graph renderer is `server/flutter_app/lib/features/ui_map/presentation/ui_map_graph.dart`.
It takes screen nodes and directed edges without any MCP dependency and can be
used in other QC views. The AstroTalk reference archive is not required for
running or rendering a map.

The [Chrome smoke test](../automation_test/ui_map_smoke.py) exercises runtime
mapping, fixture inputs, branch replay, screenshot retrieval and route lookup.

# UI map in QC and MCP

Open the target app or Chrome tab first. In QC, the UI map icon beside the
target's Live and Screenshot buttons opens the reusable graph. Both the QC
desktop client and QC Web use the same graph widget and MCP job.
Use **UI maps** in the QC toolbar to reopen saved graphs even when the target
is disconnected.

Start with **Start scan**. With no actions selected the scanner inventories the
current screen only. Supply exact visible labels or resource IDs for taps, or
enable **Try all eligible buttons** for an isolated test target. The crawler
does not classify controls by their names. It skips unlabeled or ambiguous
targets, disabled controls, and editors without configured synthetic values.
Taps can change app state. Values used to fill editors are not stored in the
graph, but a screenshot can show displayed test data. The legacy MCP
`fixtureControls` field still adds exact tap selectors like `allowedTaps`.
See [ADR-0003](../../docs/adr/0003-explicit-automation-behavior.md) for the
decision not to introduce hidden action filters.

Nodes are observed screen signatures. A directed edge records the control
and action that led from one state to another. Select a node to inspect its
screenshot, attempted and skipped controls, and shortest observed path from
entry. Use the search list when the graph gets dense. **Stop** ends a scan
after its current operation and retains the partial map. Reopen saved maps
from QC or MCP. Artifacts are persisted in `ui-maps/` under Device Bot's
configured report data directory.

At the end of a scan, `timing.json` records the elapsed time in milliseconds
and the count, total, and longest duration for each phase. Tree reads and
foreground checks can be part of branch restore or settling, so phase totals
overlap.

The action YAML supplies the first settle wait. The crawler then checks for
two matching screen signatures without repeating that initial wait.

Each attempted action runs through `robot-flow`. Its `evidence` entry links
to the generated `scenario.yaml` and, when captured, `result.png` under
`actions/<screen-id>-<control-id>/`. These include actions that kept the
same screen. Each directory also contains the flow's `report.json`.
When a Chrome state settles, the scanner records its visible CSS viewport,
the observed content extent, and which control centers are outside the
viewport. This happens before generating actions for the screen. Generated
YAML uses `scroll_into_view: true` so the runner can measure again and find
the control after scrolling.

On macOS, the crawler checks for a unique, visible vertical scroll container
in the selected window, using native AX first or the same Flutter tree already
selected for the scan. It probes up to four observed positions per direction,
reads the tree after each scroll, and verifies the original viewport before
trying controls. Controls first seen elsewhere in that viewport are kept on
the original screen and marked `initiallyOffscreen`. Their action YAML wheels
inside the measured container before resolving and tapping the control.
`nativeViewport` records the visible desktop bounds and number of newly
revealed controls. It does not estimate the full content height.

## Exploration behavior and limits

The crawler uses the selected device's accessibility tree and real input
driver. It visits newly observed states, deduplicates signatures, verifies
the route back to each branch before continuing, and bounds depth, screens,
actions and runtime. On a fresh web entry page, the crawler gives controls
time to appear and reports partial coverage if the inventory stays empty.
On web, scoped mobile and opted-in macOS fixtures, it tries short
routes through previously observed transitions and verifies each state. When web cannot restore a
branch, it restarts from the initial URL in a fresh document. It stops on an
origin change. Scoped mobile scans stop if the foreground app changes. When a
screen cannot be restored and the app is still in scope, the crawler reports
that screen's untried controls and continues with other branches. Native
navigation can use one unique accessible Back, Close,
or Cancel control, or an exact configured `backControl`. For a mobile
fixture that supports relaunch, `resetAppId` must match the open app and
lets the scanner restart it before replaying a branch. If a native screen
cannot be restored, the scan reports partial coverage rather than sending
input to an unverified screen. Desktop maps use an explicitly selected
process/window, just like desktop Live.

For a disposable macOS fixture with one window, set **Fixture bundle ID** in QC
or pass `resetAppId` to the MCP tool. It must match the selected window's app.
If known routes cannot restore a branch, the crawler quits only that process,
reopens the same app bundle without clearing data, and binds to its new window.
It verifies the restored screen before continuing and verifies each step when
replaying the recorded route. Status returns
the latest window-scoped `deviceId`. Without this option the crawler will not
quit the desktop app.
When a recorded offscreen action needs its original viewport slice, the same
opt-in recovery applies even if only the layout changed. If the recorded scroll
position still cannot be verified, the action is marked skipped with that reason.

On macOS, screenshots are limited to the selected window. If native
Accessibility exposes no named controls on entry, the job can use the Flutter
bridge for the same single-window process. It keeps that source for the whole
scan and records it in the action YAML. For a bound macOS fixture it can
discover the sandbox's Flutter registration again after reopening. Unbound
sandboxed apps may need a configured endpoint. See the
[Orbit macOS example](../../examples/flutter_app/README.md#macos-ui-map).
The default scan budget is 24 screens, 45 actions, 5 levels and 8 minutes.
Use **Scan limits** to change it for larger test targets.

This is an inventory of reachable and observable UI states, not proof of
exhaustive app coverage or a test verdict. Beyond the bounded macOS viewport
probe, lazy content, independent scroll panes, items hidden from accessibility,
permissions, external origins and conditional data may need a separate scan or
flow. If a visible scroll container is ambiguous or the scroll cap is reached,
the map reports that limitation.
Fingerprinting uses accessible control state, headings, dialogs and location.
For configured editors it tracks whether the exposed value matches the
synthetic input, which lets a filled form lead to new actions. Actual field
values, focus and geometry are excluded. Similar-looking screens can be
grouped if they expose the same signature.

Checked and selected states keep their screenshots and incoming
transitions. When their control inventory differs only in checked or
selected values, `sameControlsAs` points to the state whose controls are
explored. Enable `expandSelectionStates` to verify actions from every
combination that the scan reaches within its limits.

## MCP operations

The `mobile_ui_map` tool uses a single action argument. These are example
argument objects for `tools/call`:

```json
{"action":"start","deviceId":"chrome:<tab-id>","allowedTaps":["open-inbox","Open Settings"],"inputs":{"search-box":"sample search"},"maxScreens":30,"maxActions":60}
{"action":"status","mapId":"<map-id>"}
{"action":"route","mapId":"<map-id>","from":"s1","to":"s4"}
{"action":"screenshot","mapId":"<map-id>","screenId":"s4"}
{"action":"action_evidence","mapId":"<map-id>","screenId":"s4","controlId":"c2"}
{"action":"stop","mapId":"<map-id>"}
{"action":"list"}
```

Status returns screen IDs, controls, observations, source-only routes and
coverage notes. Copy an observed screen ID to query a shortest path. An exact
screen title or a saved source-route name/path also works. Always distinguish
`observed`, `no_observed_route`, and `source_only`.

To replay a saved action, first open the target screen shown by the graph.
For an action discovered by a native scroll, start at the viewport position
shown in that screen's entry screenshot.
Set `DEVICE_BOT_MAP_DEVICE` to the current device ID. For a fill action, set
`DEVICE_BOT_MAP_INPUT` to the synthetic value for that control. The generated
YAML stores only the variable name.

```sh
device-bot run --dry-run path/to/scenario.yaml
device-bot run path/to/scenario.yaml
```

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

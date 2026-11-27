# Architecture boundaries

For the end-to-end workflow, applications and technology map, start with the
[repository overview](../../docs/repository-overview.md) and the
[shared flow](../../README.md#how-the-repo-fits-together).

The server is organized around directional dependencies so a transport,
protocol feature, or device driver can change without absorbing unrelated
responsibilities.

```text
mcp-server (binary + connection orchestration)
        │
        ├── mcp-protocol (wire types only)
        ├── admin-api (local operator read models + SSE only)
        ├── robot-project (project/env/resource orchestration)
        ├── robot-flow / robot-report (flow execution + persistent history)
        ├── robot-stream (Live session / WebRTC infrastructure)
        ├── robot-core (portable device contracts and routing)
        └── robot-android / robot-ios-sim / robot-ios-real
            robot-macos / robot-windows / robot-chrome
                    └── depend on robot-core
```

`mcp-protocol` never starts processes, owns connection state, or knows a
device. It owns JSON-RPC/MCP serde types and protocol constants.

`mcp-server` is split by concern:

- `stdio.rs`: line-oriented process I/O; no MCP routing decisions.
- `server.rs`: method routing and lifecycle-gated tool access; no I/O loops.
- `lifecycle.rs`: valid state transitions only.
- `response.rs`: JSON-RPC response construction only.
- `main.rs`: process entrypoint only.
- `cli.rs`: pure command parsing and argument validation; no environment or I/O.
- `commands.rs`: CLI operations such as health, signing and upgrades.
- `config.rs`: resolve paths and validate bridge configuration before startup.
- `bootstrap.rs`: bridge listeners, driver assembly, serving and shutdown.
- `companion.rs`: optional Flutter companion process lifecycle.
- `http.rs`: Streamable HTTP session ownership, bearer-token validation, and
  JSON/SSE response encoding; it delegates every protocol request to `Server`.

`admin-api` owns the local `/admin/*` read-model boundary: device snapshots,
on-demand PNG screenshots, safe configuration status, and bounded activity
events. It may invoke the platform-neutral screenshot capability through the
robot registry, but it does not route MCP methods or depend on a concrete
platform driver. `mcp-server::http` composes this router at the HTTP edge and
records only transport-level activity.

`flutter_app/` is a separate Flutter client, not a Cargo member or a platform
driver. QC uses HTTP admin read models and MCP tool calls for control, plus
WebRTC for Live video. It does not invoke ADB, simctl or go-ios directly.

`robot-core` owns portable `Robot`, device, point, and error contracts. It
must not refer to `adb`, `simctl`, HTTP, or MCP message types.

The crate root re-exports its public contracts so consumers do not depend on
the internal file layout. Inside `robot-core`, keep changes with their owner:

| Module | Responsibility |
| --- | --- |
| `device.rs` | Device identity, desktop targets, screenshot options and geometry |
| `input.rs` | Portable input actions and IME commands |
| `elements.rs` | Accessibility models, classification, filtering and result limits |
| `web.rs` | Stable document/frame selectors, validation and context matching |
| `robot.rs` | Driver interface, default capability behavior and helper status |
| `registry.rs` | Device routing, session isolation and driver cleanup |
| `video.rs` | Encoded/native stream contracts and subprocess capture transport |
| `health.rs` | Read-only health reports and recovery recommendations |

Contract and registry tests in `tests.rs` exercise the root exports used by
drivers and transports. Run `cargo test -p robot-core` from `server/` for those
contracts; use `cargo check --workspace --all-targets` to verify consumers after
changing a public contract.

`ElementQuery` applies document scope, text, and element type before
`max_results`, preserving source order. Text matching is case-insensitive
across class, resource ID, label, description, and native value when present.
`apply_element_query` implements the portable policy for normalized snapshots.
macOS reuses the portable role classifier and text matching while traversing
AX, stopping after it finds the requested number of matches. Windows applies
the portable query to a bounded UIA or MSAA snapshot. It forwards a caller's
result limit to the helper for unfiltered requests and zero-result queries.
The iOS adapters also pass filters to XCTest to bound native traversal.

The native iOS Simulator query forwards system keyboard exclusion to XCTest
and checks its acknowledgment. Other element sources return
`RobotError::Unsupported` for that explicit request before reading the tree.
The option controls tree membership. Keyboard visibility is managed separately
by the app and OS. See the [platform matrix](platform-parity.md) for each
driver's element source, document scope, and runtime prerequisites.

`robot-flow::runner` owns flow lifecycle, cancellation, step deadlines, device
scope and the MCP session. Its private modules share that state while keeping
different reasons for change separate:

- `runner/preflight.rs` validates every compiled branch before tool execution.
- `runner/actions.rs` dispatches validated actions.
- `runner/elements.rs` owns discovery, bounded waits, assertions, focus and
  document scoping.
- `runner/arguments.rs` builds the MCP payloads shared by validation and execution.
- `runner/artifacts.rs` owns evidence paths, snapshots, recording cleanup and
  secret redaction.

Keep tool payload construction in `arguments.rs` so preflight and execution use
the same contract. Child-module methods are visible only within the runner;
callers continue to use `FlowRunner`, `FlowReport` and `runner::filter_elements`.
Run `cargo test -p robot-flow` for the mocked MCP workflows, CLI subprocesses,
schema contracts and evidence regressions before changing execution behavior.

`robot-flow::outcome` owns typed execution errors and the stable report status
values. MCP clients return `FlowError`; error context and cleanup preserve the
original kind. Classify deadlines and unsupported observations where they occur,
and tag transport failures before they cross an assertion step. Never infer an
error kind from words in its message. Opaque MCP tool failures remain execution
failures unless their kind is supplied explicitly. Report fields retain their
existing string values for JSON consumers and persisted history.

In flow templates, `${name}` resolves a variable and `$${name}` is the literal
text `${name}`. Exporters escape every recorded `${` opener, including an
unfinished `${...` typed by a user. Variable values are opaque and are not
expanded again. The export-only encoder lives in QC's `core/models/flow_yaml.dart`;
normal MCP JSON retains the original values. CI exports real Flutter recordings
and replays them through the Rust compiler and a stdio MCP fixture on Linux and
Windows, comparing the forwarded text with the original recorded input.

Each platform crate depends on `robot-core` and owns its external process
commands, output parsers, and driver-specific tests. Tool-to-driver adaptation
belongs in the server handler module for each tool family.

`robot-ios-real` owns the `go-ios` command boundary, signed XCTest control,
and the USB/Wi-Fi screen capture paths. It is independent of `robot-ios-sim`, whose
`simctl` lifecycle and recording state have different failure modes. The
simulator driver invokes the repository's XCTest UI-test runner for synthesized
input and accessibility-tree reads. The project at
`crates/robot-ios-sim/xctest-runner` runs through `xcodebuild` and has no
separately installed companion or client dependency.

Tool definitions live in `mcp-server::tools`; each family (device, app,
screen, input, crash) gets its own handler module. The initial Android slice
uses `tools/device.rs`, `tools/app.rs`, `tools/navigation.rs`,
`tools/input.rs`, `tools/recording.rs`, `tools/screen.rs`, and `tools/ui.rs`, with generic request
parsing in `tools/arguments.rs`. The router stays a small method-to-handler
dispatch table rather than a business-logic container.

Platform capabilities, implementation limits, and the checks for shared MCP
behavior are tracked in [platform-parity.md](platform-parity.md).

### Platform adapter layout

Each driver keeps its `impl Robot` in `adapter.rs` and delegates to private
modules that own native operations. The crate root owns the public driver type,
construction, and re-exports. The registered physical iOS driver follows this
layout inside `xctest_robot/`.

- Portable traits, enums, and errors live in their owning `robot-core` modules.
  Drivers keep native payload types with their decoder. Android and iOS use
  `protocol.rs`. Chrome owns CDP transport in `connection.rs` and DOM
  normalization in `dom.rs`. Windows owns UIA and MSAA in `accessibility/`.
- Driver tests live in `tests/` alongside the adapter or beside a private
  native module when they exercise its implementation. Cross-platform tests
  call the shared `Robot` interface through the server test suite.

Inside `robot-macos`, native implementation details live behind private modules:

- `ffi.rs` declares framework bindings and `CfOwned` reference cleanup.
- `accessibility.rs` owns AX attribute reads, traversal, focus and hit testing.
- `windows.rs` owns native window discovery and portable app/window projections.
- `live.rs`, `screenshot.rs` and `pointer.rs` own their respective capture/input
  lifecycles and import native dependencies explicitly.
- `tests/` exercises driver behavior. The real desktop checks in
  `tests/screenshot.rs` require an interactive session and OS permissions.

The `ffi` and `accessibility` modules compile only on macOS. Window projection
helpers also compile under tests on other hosts. Keep this distinction when
moving native code so Linux/Windows builds remain useful for portable tests.
Run `cargo test -p robot-macos --lib` for automated regressions and use a Linux
target check to verify the non-macOS configuration.

Inside `robot-windows`, `src/accessibility/` contains the UIA PowerShell
template and the optional MSAA provider. Both project native nodes into the
same payload for the Rust decoder. `live.rs` pins window-scoped reads to the
selected HWND and its owner. The driver applies the portable query on the
decoded snapshot.

### Browser observation

Event names and fields follow the Chrome DevTools Protocol
[Runtime](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/) and
[Network](https://chromedevtools.github.io/devtools-protocol/tot/Network/) domains.

`mobile_start_observation` attaches an independent CDP connection to a page in
the current Chrome case context. Start it before the action; poll using
`mobile_poll_observation` and an `afterSequence` cursor, then stop with
`mobile_stop_observation`. Each observer retains at most 1,024 records and the
batch reports cumulative `dropped` records and whether its connection closed.
Each session owns at most 16 observers, which are stopped when it closes.
Sequence and elapsed milliseconds are local to the observer. A frame ID is
included when CDP supplies one or associates an execution context with a frame;
it can be absent. Redirects appear as request events with `redirectStatus`.

Capture currently covers the attached page target only, not out-of-process iframe
or worker targets. Events may be missed before start, on disconnect, or when the
ring overflows; native device network observation is unsupported. Network events
include request/response metadata, failures, WebSocket frame direction/encoded
payload size (binary payloads are represented as base64 by CDP), and
SSE message size, but no headers or bodies. Console and exceptions contain
metadata by default. `includeText` explicitly enables bounded console string
arguments and exception descriptions; `redact` supplies up to 64 exact secrets
to replace in all emitted event-data strings. This is not comprehensive secret detection: callers
must supply all relevant secrets and opt in only when appropriate. URLs omit
user info, query and fragment. No response body or native HTTPS interception is
provided. Poll exposes records for future wait/assert logic; it does not assert.
On disconnect, `closed` and `terminalReason` report that collection ended;
they do not quantify events lost after disconnect.

### Device capability preflight

`mobile_get_capabilities` accepts a selected `deviceId` and reports the server's
`hostOs` and per-operation `status` and `reason`. `available` means the driver
implements the operation and its current read-only prerequisite checks passed;
it does not guarantee a later action, target application, or recorded evidence
will succeed. `blocked` identifies a missing prerequisite, `unsupported` an
unimplemented operation, and `unknown` a capability whose readiness was not
established. Required preflight must reject all three. Checks do not grant or
prompt for permissions, launch applications, inject input, or start recordings.
Native application network observation is not implemented, and Chrome observation
is limited to the DevTools target described above. `hostOs` identifies the server
host, not necessarily the host of a remotely configured Chrome endpoint.

## Rust implementation and review

Choose the owning crate before designing the Rust API. Use the existing
`Robot` trait and portable types at the driver boundary. Keep concrete
types inside a crate when there is a single implementation. Add an
abstraction when callers need the contract to support another implementation
or an independently testable boundary.

- Use typed errors in `robot-core` and `robot-flow`. Drivers translate
  native failures into `RobotError`, and the MCP and reporting edges map
  errors into their public formats. Keep public signatures, JSON/YAML fields,
  and persisted status strings compatible unless the change calls for a
  contract revision.
- Put native commands, FFI, and cleanup in the owning adapter. Document the
  invariants of changed unsafe code. Keep session and cancellation ownership
  at the existing boundary, including shared ownership where the registry or
  session lifecycle requires it.
- Review the dependency direction and the data and error path before style
  details. Test portable rules in their owning crate and use native fixtures
  to test adapter translation. Use the workspace checks in [AGENTS.md](../../AGENTS.md#build-and-validation).

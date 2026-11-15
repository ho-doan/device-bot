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
        ├── robot-flow / robot-report (flow execution + persistent history)
        ├── robot-stream (Live session / WebRTC infrastructure)
        └── robot-core (device contracts only)
                │
                └── robot-android / robot-ios-sim / robot-ios-real
                    robot-macos / robot-windows / robot-chrome
```

`mcp-protocol` never starts processes, owns connection state, or knows a
device. It owns JSON-RPC/MCP serde types and protocol constants.

`mcp-server` is split by concern:

- `stdio.rs`: line-oriented process I/O; no MCP routing decisions.
- `server.rs`: method routing and lifecycle-gated tool access; no I/O loops.
- `lifecycle.rs`: valid state transitions only.
- `response.rs`: JSON-RPC response construction only.
- `main.rs`: process entrypoint only.
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

Each platform crate owns its external process commands, output parsers, and
driver-specific tests. `robot-android` may depend on `robot-core`, but no
platform driver may depend on `mcp-server`; tool-to-driver adaptation belongs
in narrowly scoped server handler modules added with each tool family.

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

Temporary platform capability gaps are tracked in
[platform-parity.md](platform-parity.md). The final parity pass owns alignment
of shared MCP behavior rather than making a phase-specific handler a
cross-platform policy container.

Inside `robot-macos`, `lib.rs` owns the driver, process commands and session
state. Native implementation details live behind private modules:

- `ffi.rs` declares framework bindings and `CfOwned` reference cleanup.
- `accessibility.rs` owns AX attribute reads, traversal, focus and hit testing.
- `windows.rs` owns native window discovery and portable app/window projections.
- `live.rs`, `screenshot.rs` and `pointer.rs` own their respective capture/input
  lifecycles and import native dependencies explicitly.
- `tests.rs` exercises driver behavior; `screenshot_tests.rs` retains the real
  desktop checks, which require an interactive session and OS permissions.

The `ffi` and `accessibility` modules compile only on macOS. Window projection
helpers also compile under tests on other hosts. Keep this distinction when
moving native code so Linux/Windows builds remain useful for portable tests.
Run `cargo test -p robot-macos --lib` for automated regressions and use a Linux
target check to verify the non-macOS configuration.

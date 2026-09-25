# Mobile MCP Rust server

The Rust server is intentionally independent of the Flutter plugin at the
repository root. It implements the MCP transport and device abstractions; the
Flutter companion can be added later through a local admin API without being
part of the MCP request path.

## Current milestone

Phases 0–3 and the Phase 5 Flutter companion MVP are implemented:

- a Cargo workspace with protocol, server, core robot, Android-driver, and iOS Simulator-driver crates;
- newline-delimited JSON-RPC 2.0 over stdio;
- Streamable HTTP on an explicit `--listen HOST:PORT` endpoint, with JSON or
  single-message SSE responses and optional bearer authentication;
- MCP `initialize` / `notifications/initialized` lifecycle enforcement;
- an ADB driver with a mockable command runner and deterministic `adb devices -l` parser;
- Android device/app/input/screen/accessibility tools, plus iOS Simulator boot, screenshot, and MP4 screen-recording support.
- a separate local admin API and Flutter companion source tree for inspecting
  connected devices, auth/configuration state, and recent MCP activity.

Real iOS-device deployment is intentionally deferred until the final phase.
The existing `robot-ios-real` crate is only an isolated adapter foundation; it
is not part of the current deployment target.

The supported MCP protocol version is `2025-11-25`.

The ownership and dependency rules that keep this workspace from accumulating
a central "god file" are documented in [architecture.md](docs/architecture.md).

## Run

```bash
cargo run -p mobile-mcp-server
```

The binary executes `adb` and `xcrun` from `PATH`. Set `ANDROID_ADB_PATH` or
`XCRUN_PATH` to use a specific installation. Set `GO_IOS_PATH` when the
`go-ios` binary is not named `ios`. Recording artifacts are written to
`artifacts/` by default; set `MOBILE_MCP_ARTIFACT_DIR` to override it.

The process accepts one JSON-RPC message per stdin line and writes one response
per stdout line. Diagnostic output is reserved for stderr.

## HTTP transport and auth

Run an HTTP listener explicitly; prefer a loopback address unless a reverse
proxy supplies the network boundary:

```bash
MOBILEMCP_AUTH=replace-with-a-secret \
  cargo run -p mobile-mcp-server -- --listen 127.0.0.1:3337
```

`POST /mcp` implements the `2025-11-25` session lifecycle. Send the token as
`Authorization: Bearer <token>` when `MOBILEMCP_AUTH` is non-empty. An
`initialize` response returns `Mcp-Session-Id`; repeat it for subsequent calls,
send `notifications/initialized`, then call tools. Responses are JSON by
default, or one SSE `message` event when the client sends
`Accept: text/event-stream`. `DELETE /mcp` with the same session header closes
the in-memory session.

## Local admin API and Flutter companion

The same explicit HTTP listener also exposes local operator endpoints:

| Endpoint | Purpose |
| --- | --- |
| `GET /admin/devices` | Current devices from the robot registry |
| `GET /admin/devices/{device_id}/screenshot` | Current device PNG for the companion viewer |
| `GET /admin/config` | Safe-to-display auth and log-buffer status |
| `GET /admin/logs` | Bounded, recent MCP request activity |
| `GET /admin/logs/stream` | Future activity as server-sent events |

They share the HTTP listener's bearer-token policy. The Flutter companion
source lives in [`flutter_app/`](flutter_app/); it talks only to these admin
endpoints, never to a platform driver or directly to ADB. It can also launch
and stop a local `mobile-mcp-server` child process for convenience, but never
stops an existing server it did not create.

For example:

```text
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

## Android tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `mobile_list_available_devices` | `{}` | `structuredContent.devices` with id, name, platform, and state |
| `mobile_boot_device` | `{ "deviceId": "iOS-simulator-UDID" }` | Boot an iOS Simulator and wait until ready |
| `mobile_launch_app` | `{ "deviceId": "emulator-5554", "appId": "com.android.chrome" }` | Launch confirmation |
| `mobile_install_app` | `{ "deviceId": "emulator-5554", "apkPath": "/host/app.apk" }` | Install/replace APK confirmation |
| `mobile_uninstall_app` | `{ "deviceId": "emulator-5554", "appId": "com.example.app" }` | Uninstall confirmation |
| `mobile_open_url` | `{ "deviceId": "emulator-5554", "url": "https://…", "appId": "com.android.chrome" }` | URL-open confirmation; `appId` is optional |
| `mobile_type_keys` | `{ "deviceId": "emulator-5554", "text": "babyshake" }` | Text-input confirmation |
| `mobile_take_screenshot` | `{ "deviceId": "emulator-5554" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_click_on_screen_at_coordinates` | `{ "deviceId": "emulator-5554", "x": 100, "y": 240 }` | Text confirmation |
| `mobile_swipe_on_screen` | `{ "deviceId": "emulator-5554", "startX": 100, "startY": 800, "endX": 100, "endY": 200 }` | Swipe confirmation; optional `durationMs` |
| `mobile_list_elements_on_screen` | `{ "deviceId": "emulator-5554" }` | `structuredContent.elements` from the current accessibility tree |

## iOS Simulator tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `mobile_take_screenshot` | `{ "deviceId": "iOS-simulator-UDID" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_start_screen_recording` | `{ "deviceId": "iOS-simulator-UDID" }` | Starts a simulator MP4 artifact |
| `mobile_stop_screen_recording` | `{ "deviceId": "iOS-simulator-UDID" }` | Stops recording and returns the saved MP4 path |

## Real iOS devices (adapter foundation only)

`robot-ios-real` is reserved for the final real-iOS phase. It uses `go-ios` for discovery and for a pre-running DeviceKit
or WebDriverAgent UI backend. It supports device discovery, screenshots, UI
source normalization, tap/swipe/text input, app launch, installation, and URL
launching through the existing MCP tools. A real-device run requires a paired,
trusted iPhone plus the signed automation runner; absent prerequisites produce
an MCP driver error and never fall back to controlling the host UI.

Device-driver failures and invalid tool arguments return a normal MCP tool
result with `isError: true`, so clients can present actionable retry feedback.

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

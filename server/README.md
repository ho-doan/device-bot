# Mobile MCP Rust server

The Rust server is intentionally independent of the Flutter plugin at the
repository root. It implements the MCP transport and device abstractions; the
Flutter companion can be added later through a local admin API without being
part of the MCP request path.

## Current milestone

Phases 0–2 are implemented:

- a Cargo workspace with protocol, server, core robot, Android-driver, and iOS Simulator-driver crates;
- newline-delimited JSON-RPC 2.0 over stdio;
- MCP `initialize` / `notifications/initialized` lifecycle enforcement;
- an ADB driver with a mockable command runner and deterministic `adb devices -l` parser;
- Android device/app/input/screen/accessibility tools, plus iOS Simulator boot, screenshot, and MP4 screen-recording support.

The supported MCP protocol version is `2025-11-25`.

The ownership and dependency rules that keep this workspace from accumulating
a central "god file" are documented in [architecture.md](docs/architecture.md).

## Run

```bash
cargo run -p mobile-mcp-server
```

The binary executes `adb` and `xcrun` from `PATH`. Set `ANDROID_ADB_PATH` or
`XCRUN_PATH` to use a specific installation. Recording artifacts are written
to `artifacts/` by default; set `MOBILE_MCP_ARTIFACT_DIR` to override it.

The process accepts one JSON-RPC message per stdin line and writes one response
per stdout line. Diagnostic output is reserved for stderr.

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

Device-driver failures and invalid tool arguments return a normal MCP tool
result with `isError: true`, so clients can present actionable retry feedback.

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

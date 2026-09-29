# Device Bot

The Rust server is intentionally independent of the Flutter plugin at the
repository root. It implements the MCP transport and device abstractions; the
Flutter companion can be added later through a local admin API without being
part of the MCP request path.

## Current milestone

Phases 0–3 and the Phase 5 Flutter companion MVP are implemented:

- a Cargo workspace with protocol, server, core robot, Android/iOS drivers, native macOS/Windows drivers, and a Chrome DevTools driver;
- newline-delimited JSON-RPC 2.0 over stdio;
- Streamable HTTP on an explicit `--listen HOST:PORT` endpoint, with JSON or
  single-message SSE responses and optional bearer authentication;
- MCP `initialize` / `notifications/initialized` lifecycle enforcement;
- an ADB driver with a mockable command runner and deterministic `adb devices -l` parser;
- Android device/app/input/screen/accessibility tools, plus iOS Simulator boot,
  screenshot, MP4 screen-recording, and bundled-XCTest input/accessibility support.
- a separate local admin API and Flutter companion source tree for inspecting
  connected devices, auth/configuration state, and recent MCP activity.

The server also registers the real-iOS XCTest, macOS, Windows, and Chrome
drivers. Registration does **not** mean that every driver has feature parity;
the platform matrix below is the source of truth.

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
`artifacts/` by default; set `DEVICE_BOT_ARTIFACT_DIR` to override it.

The process accepts one JSON-RPC message per stdin line and writes one response
per stdout line. Diagnostic output is reserved for stderr.

## HTTP transport and auth

Run an HTTP listener explicitly; prefer a loopback address unless a reverse
proxy supplies the network boundary:

```bash
DEVICE_BOT_AUTH=replace-with-a-secret \
  cargo run -p mobile-mcp-server -- --listen 127.0.0.1:3337
```

`POST /mcp` implements the `2025-11-25` session lifecycle. Send the token as
`Authorization: Bearer <token>` when `DEVICE_BOT_AUTH` is non-empty. An
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

## Platform support and known gaps

`mobile_launch_app` is the platform operation: use it to launch an installed
application (including Chrome) on Android, iOS, macOS, or Windows. Browser
navigation belongs to the **Chrome DevTools** robot, not to an operating-system
platform robot. In particular, a client should not use `mobile_open_url` as a
claim that macOS, Windows, Android, or iOS browser automation is supported.

| Target | Implemented operations | Not implemented / prerequisite |
| --- | --- | --- |
| Android device or emulator | discovery, app install/uninstall/launch, screenshot, MP4 recording, tap, text, swipe, accessibility elements | Requires ADB and a connected or booted device. Chrome UI flow still requires the Chrome DevTools robot. |
| iOS Simulator | boot, app install/uninstall/launch, screenshot, MP4 recording, tap, text, swipe, XCTest elements | Requires Xcode and the bundled XCTest runner to be buildable for the target app. Chrome UI flow still requires the Chrome DevTools robot. |
| Real iOS device | CoreDevice discovery, install/uninstall, app launch, XCTest screenshot/input/swipe/elements, and QuickTime MP4 recording when the signed XCTest agent is reachable | Recording uses QuickTime's iPhone capture device and therefore requires the trusted iPhone to be connected by USB. A wireless device is discoverable, but its XCTest agent must bind to a LAN-reachable MCP host; the default loopback bind is not sufficient. |
| macOS host | screenshot, MP4 recording, launch app, tap, text, accessibility elements | No installer/uninstaller or drag. Requires Screen Recording permission for recording and Accessibility permission for input. |
| Windows host | screenshot, launch app, tap, text, drag | No recording, installer/uninstaller, or accessibility-tree discovery. The temporary OmniParser coordinate fallback for Flutter is not implemented. Requires an interactive Windows session and UI-automation access. |
| Chrome DevTools page | screenshot, MP4 recording, click, text, drag, DOM elements, navigate the attached page | It requires a reachable DevTools endpoint. |

The compatibility methods named `open_url` still exist in several platform
drivers while this migration is underway. They are outside the supported
platform contract above and must not be relied on; only a `chrome:<target-id>`
device is the supported destination for `mobile_open_url`.

### Required Chrome workflow

The intended end-to-end sequence is:

1. Call `mobile_launch_app` on the platform device to open Chrome.
2. Ensure that the browser exposes its configured DevTools endpoint, then
   select its `chrome:<target-id>` device from `mobile_list_available_devices`.
3. Use the platform accessibility tree for Chrome's native controls such as
   **New Tab** and the address bar; use the Chrome robot for page DOM elements,
   search results, links, playback controls, and Chrome recording.

## Device tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `mobile_list_available_devices` | `{}` | `structuredContent.devices` with id, name, platform, and state |
| `mobile_boot_device` | `{ "deviceId": "iOS-simulator-UDID" }` | Boot an iOS Simulator and wait until ready |
| `mobile_launch_app` | `{ "deviceId": "emulator-5554", "appId": "com.android.chrome" }` | Launch confirmation |
| `mobile_install_app` | `{ "deviceId": "emulator-5554", "apkPath": "/host/app.apk" }` | Install/replace APK confirmation |
| `mobile_uninstall_app` | `{ "deviceId": "emulator-5554", "appId": "com.example.app" }` | Uninstall confirmation |
| `mobile_list_installed_apps` | `{ "deviceId": "device-id" }` | Installed-app inventory for Android, macOS, iOS, or Windows; the result is structured JSON |
| `mobile_type_keys` | `{ "deviceId": "emulator-5554", "text": "babyshake" }` | Text-input confirmation |
| `mobile_take_screenshot` | `{ "deviceId": "emulator-5554" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_click_on_screen_at_coordinates` | `{ "deviceId": "emulator-5554", "x": 100, "y": 240 }` | Text confirmation |
| `mobile_swipe_on_screen` | `{ "deviceId": "emulator-5554", "startX": 100, "startY": 800, "endX": 100, "endY": 200 }` | Swipe confirmation; optional `durationMs` |
| `mobile_list_elements_on_screen` | `{ "deviceId": "emulator-5554", "filter": "Save", "maxResults": 20 }` | `structuredContent.elements` matching the optional case-insensitive filter, bounded by `maxResults` |

## iOS Simulator tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `mobile_take_screenshot` | `{ "deviceId": "iOS-simulator-UDID" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_click_on_screen_at_coordinates` | `{ "deviceId": "iOS-simulator-UDID", "x": 100, "y": 240 }` | Bundled XCTest touch at the screen point |
| `mobile_type_keys` | `{ "deviceId": "iOS-simulator-UDID", "text": "hello" }` | Bundled XCTest keyboard input |
| `mobile_swipe_on_screen` | `{ "deviceId": "iOS-simulator-UDID", "startX": 100, "startY": 700, "endX": 100, "endY": 200 }` | Bundled XCTest touch swipe; optional `durationMs` |
| `mobile_list_elements_on_screen` | `{ "deviceId": "iOS-simulator-UDID", "filter": "Baby Shark", "maxResults": 10 }` | XCTest query is filtered natively and bounded before element materialization for fast web-view queries |
| `mobile_start_screen_recording` | `{ "deviceId": "iOS-simulator-UDID" }` | Starts a simulator MP4 artifact |
| `mobile_stop_screen_recording` | `{ "deviceId": "iOS-simulator-UDID" }` | Stops recording and returns the saved MP4 path |

### Simulator UI automation

`simctl` does not expose touch, keyboard, swipe, or accessibility-tree
operations. The repository includes a small XCTest UI-test runner at
`crates/robot-ios-sim/xctest-runner`, so no external client, companion, or
Homebrew tap is required. It runs through the host's `xcodebuild`; lifecycle, app
management, screenshots, and recording continue to use `xcrun simctl`.

Launch the target app with `mobile_launch_app` before the UI tools. The runner
targets that bundle ID and returns normalized accessibility elements.

## Real iOS devices

The registered real-device driver uses Apple CoreDevice for paired-device
discovery and app lifecycle, plus the signed XCTest agent for screenshots and
UI automation. A real-device run requires a paired, trusted iPhone and an
agent connection back to the MCP host. Bind `DEVICE_BOT_IOS_AGENT_BIND` to a LAN
address or `0.0.0.0` and set `DEVICE_BOT_IOS_AGENT_TOKEN`. `127.0.0.1` only
works for a local host process, not an iPhone. When the bind address is
`0.0.0.0`, the server advertises the Mac's `en*` IPv4 addresses, skipping
loopback and link-local. Set `DEVICE_BOT_IOS_AGENT_HOSTS` to override that
list. Do not advertise the CoreDevice or go-ios tunnel address: that tunnel
is how the Mac reaches the phone, and the phone's callback does not arrive
through it. On first contact the signed runner accepts the system Local
Network sheet (Allow / Cho phép). If iOS will not show that sheet again, the
runner opens Settings → Privacy & Security → Local Network and turns on
Device Bot.

`mobile_list_installed_apps` is implemented by each native driver: Android uses
`adb shell pm list packages -f`, macOS uses `system_profiler
SPApplicationsDataType -json`, iOS Simulator uses `xcrun simctl listapps` plus
`plutil`, real iOS uses `xcrun devicectl device info apps --include-all-apps`,
and Windows reads the standard uninstall registry keys via PowerShell. The
paired iOS device must be available to CoreDevice; its XCTest agent is not
required for this inventory operation.

Real-device screen recording uses QuickTime Player as a host-side fallback. The
trusted iPhone must be connected by USB and visible as a `Screen` source under
QuickTime's Movie Recording device menu. The server stops the recording through
QuickTime, reads its autosave MOV, and uses `ffmpeg` to produce the MP4 artifact.
The first run may ask for Automation/Accessibility permission. Set
`DEVICE_BOT_IOS_QUICKTIME_DEVICE` when the device display name is not detected
automatically, and set `DEVICE_BOT_FFMPEG_PATH` when `ffmpeg` is not on `PATH`.

The Safari example supports a real iPhone after the signed XCTest agent is
running. Use the same token and paired-device UDID for the agent and example:

```bash
export DEVICE_BOT_IOS_DEVICE=<paired-iPhone-UDID>
export DEVICE_BOT_IOS_AGENT_TOKEN=<long-random-token>
server/tool/ios_xctest_agent.sh build  # first time, or after runner changes
server/tool/ios_xctest_agent.sh start

DEVICE_ID="$DEVICE_BOT_IOS_DEVICE" \
DEVICE_BOT_IOS_AGENT_TOKEN="$DEVICE_BOT_IOS_AGENT_TOKEN" \
server/examples/example_safari.sh ios-real
```

The example binds the bridge to `0.0.0.0:18787` by default, records the real
iPhone through QuickTime when `RECORD_SCREEN=auto` or `1`, and uses the
real-device XCTest driver's native filtered accessibility queries for Safari
controls. Set `RECORD_SCREEN=0` to disable recording, or set
`DEVICE_BOT_IOS_AGENT_HOSTS` if the agent cannot discover a LAN address
automatically.

### Known issue — Codex MCP on a real iPhone (deferred)

As of 2026-09-29, a direct Codex/MCP run against a paired iPhone can discover
the device, list installed apps, and make a QuickTime recording, but UI
automation remains blocked until the XCTest agent completes its callback to
the bridge. The observed device-side error was `Local network prohibited`; an
IPv6 CoreDevice-tunnel retry also produced no agent request. Do not treat a
successful device discovery or recording as a successful UI-automation run.

The Safari example is not equivalent to an arbitrary Codex MCP configuration:
it starts this repository's `mobile-mcp-server`, binds the agent bridge to a
LAN-reachable address, supplies a shared token, and polls the agent before
automation. A Codex configuration that starts another package (for example,
`@mobilenext/mobile-mcp`) does not exercise this server or its real-iOS agent.
Likewise, the server default of `127.0.0.1:18787` is reachable only from the
Mac, not from the iPhone.

There is no environment-name mismatch. The launcher and
`server/tool/ios_xctest_agent.sh` export `TEST_RUNNER_DEVICE_BOT_HOSTS`,
`TEST_RUNNER_DEVICE_BOT_PORT`, and `TEST_RUNNER_DEVICE_BOT_TOKEN`. Xcode strips
the `TEST_RUNNER_` prefix, and the XCTest runner reads `DEVICE_BOT_HOSTS`,
`DEVICE_BOT_PORT`, and `DEVICE_BOT_TOKEN`. `xcodebuild` stdout and stderr are
appended under the runner derived data `Logs/Agent` directory.

A non-loopback agent bind requires `DEVICE_BOT_IOS_AGENT_TOKEN`. Without it
the bridge would accept every caller on that interface.

Remediation and verification:

1. Configure Codex to start this repository's `mobile-mcp-server` with
   `DEVICE_BOT_IOS_AGENT_BIND` on a LAN interface or `0.0.0.0`, and the same
   `DEVICE_BOT_IOS_AGENT_TOKEN` used by the XCTest agent. Never commit that
   token. A package other than this server does not exercise this agent.
2. Rebuild and reinstall the signed XCTest runner after runner changes. The
   runner accepts the Local Network sheet, or opens that Settings page when
   iOS will not ask again. That permission is part of this callback; a tunnel
   address is not a substitute for it.
3. Verify `mobile_get_ui_runner_status` returns `ready: true` before calling
   any UI tool. Only then validate launch, element dump, tap, text input, and
   screenshot on the physical device.

Device-driver failures and invalid tool arguments return a normal MCP tool
result with `isError: true`, so clients can present actionable retry feedback.

## Desktop and Chrome robots

The macOS and Windows drivers expose their local host only on their respective
operating systems. `mobile_launch_app` opens an installed native application;
Windows also supports drag gestures. macOS requires Accessibility permission
for pointer/keyboard input. Neither desktop driver currently installs apps.
Windows also lacks recording, native element discovery, and the planned
OmniParser coordinate fallback.

Chrome is exposed when a DevTools endpoint is available. Start Chrome with
remote debugging, for example:

```sh
google-chrome --remote-debugging-port=9222
```

Set `DEVICE_BOT_CHROME_DEBUG_ADDRESS` to use a different endpoint. Each debuggable
page appears as a `chrome:<target-id>` device and supports screenshots, mouse/keyboard
input, URL navigation, and DOM-backed element discovery.

`mobile_start_screen_recording` on macOS writes an MP4 from AVFoundation and
requests Screen Recording permission automatically. The first request can require
the user to approve macOS's prompt and retry. Chrome recording also writes an MP4
from DevTools screenshots. Both recording drivers require `ffmpeg` (set
`DEVICE_BOT_FFMPEG_PATH` when it is not on `PATH`).
macOS recording uses AVFoundation's named `Capture screen 0` source rather than
a numeric device index, so a connected iPhone/Continuity camera cannot be
selected accidentally. Set `DEVICE_BOT_MACOS_SCREEN_DEVICE` when recording a
different display.

Install the recording dependency through the server itself on macOS:

```sh
cargo run -p mobile-mcp-server -- --install-chrome-recording-tools
```

You can request the macOS permission before starting the server:

```sh
cargo run -p mobile-mcp-server -- --request-screen-recording-permission
```

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

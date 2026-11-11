# Device Bot

The Rust server implements the MCP transport and device abstractions; the
optional Flutter companion is an MCP client that uses the same `/mcp` request
path as Codex or any other MCP client.

## Current milestone

Phases 0–3 and the Phase 5 Flutter companion MVP are implemented:

- a Cargo workspace with protocol, server, core robot, Android/iOS drivers, native macOS/Windows drivers, and a Chrome DevTools driver;
- newline-delimited JSON-RPC 2.0 over stdio;
- Streamable HTTP on an explicit `--listen HOST:PORT` endpoint, with JSON or
  single-message SSE responses and an auto-generated bearer token by default;
- MCP `initialize` / `notifications/initialized` lifecycle enforcement;
- an ADB driver with a mockable command runner and deterministic `adb devices -l` parser;
- Android device/app/input/screen/accessibility tools, plus iOS Simulator boot,
  screenshot, MP4 screen-recording, and bundled-XCTest input/accessibility support.
- a separate local admin API for diagnostics, plus an optional Flutter MCP
  companion source tree for manual device control and YAML flow authoring.

The server also registers the real-iOS XCTest, macOS, Windows, and Chrome
drivers. Registration does **not** mean that every driver has feature parity;
the platform matrix below is the source of truth.

The supported MCP protocol version is `2025-11-25`.

The ownership and dependency rules that keep this workspace from accumulating
a central "god file" are documented in [architecture.md](docs/architecture.md).

## Run

The installed suite has one entry point:

```sh
device-bot                       # Start the web app if needed, then open the browser
device-bot start                 # Ensure the background web app is running
device-bot qc                    # Open the QC desktop app; ensure local MCP is running
device-bot run scenario.yaml     # Run a YAML scenario
device-bot mcp                   # MCP over stdio, for MCP clients
device-bot mcp --listen 127.0.0.1:3337
```

The web app listens at `http://127.0.0.1:8765`. It runs independently of the
terminal, so closing PowerShell does not stop it. Repeating `start` reuses a
healthy report service; if it has stopped, `start` launches it again. Running
`device-bot` always opens the browser, including when the service is already up.
History is stored in `~/.device-bot/reports`, independent of the current working
directory. Set `DEVICE_BOT_REPORT_DATA_DIR` to override it before starting the
service. Startup diagnostics are written to `server.log` in that directory.
Set `DEVICE_BOT_REPORT_PORT` before starting to use another loopback port;
the default is 8765.

### QC desktop app

For a standalone macOS QC build, unzip `device-bot-qc-v<VERSION>-aarch64-apple-darwin.zip`,
move **Device Bot QC.app** to Applications, and open it. This package includes
QC, the native MCP server, the YAML/report tools, and XCTest assets; Flutter and
Rust are not needed on the receiving Mac. The current local build supports
Apple Silicon and macOS 12 or newer. Windows packaging will follow separately.
The app is ad-hoc signed, not notarized: if macOS blocks the first launch, use
System Settings → Privacy & Security → Open Anyway for this app.

`device-bot qc` opens the optional Flutter QC desktop app. It starts a detached
MCP server at `http://127.0.0.1:3337` if needed and reuses a running server.
Credentials are discovered locally; no manual Bearer token entry is needed.
MCP logs are written to `~/.device-bot/mcp/server.log`. The server keeps running
after the terminal or QC closes. An unrelated service or older Device Bot on
port 3337 must be stopped first.

The CLI release bundle currently does not include the QC app. Build it once
from this repository on a machine with Flutter and the platform build tools:

```sh
cd server/flutter_app
flutter pub get
flutter build macos       # On Windows: flutter build windows
cd ../..
device-bot qc
```

QC is found beside the CLI, in `/Applications` or `~/Applications` on macOS,
or in the local Flutter project's Release/Debug build output. For a custom
location, set `DEVICE_BOT_QC_APP` to the `.app` bundle on macOS or the executable
on Windows. Keep the Windows executable together with its Flutter bundle files.
`DEVICE_BOT_FLUTTER_APP` can point to a different companion source directory.
The command opens the existing build and does not download Flutter or build QC.

To produce the standalone app on a build machine (Python 3.12+ required for
packaging):

```sh
cargo build --manifest-path server/Cargo.toml --release --locked \
  -p device-bot -p robot-flow -p robot-report
(cd server/flutter_app && FLUTTER_XCODE_ARCHS=arm64 flutter build macos --release --build-name=0.1.9)
python3 scripts/package_qc_macos.py --binary-dir server/target/release \
  --qc-app server/flutter_app/build/macos/Build/Products/Release/mobile_mcp_companion.app \
  --target aarch64-apple-darwin --version 0.1.9 --output artifacts/qc-release
```

The ZIP includes a launcher that starts MCP before opening QC. A SHA-256 file
is generated beside it. Launch failures are recorded in `~/.device-bot/qc-launch.log`.

From source, build all three bundled executables before using the launcher:

```bash
cargo build -p device-bot -p robot-flow -p robot-report
cargo run -p device-bot -- mcp
```

The binary resolves `adb` from `ANDROID_ADB_PATH`, Android SDK environment variables,
the usual home SDK directories, or `PATH`. `xcrun` uses `PATH`; set `XCRUN_PATH`
to use a specific installation. Set `GO_IOS_PATH` when the
`go-ios` binary is not named `ios`. Recording artifacts are written to
`artifacts/` by default; set `DEVICE_BOT_ARTIFACT_DIR` to override it.

The process accepts one JSON-RPC message per stdin line and writes one response
per stdout line. Diagnostic output is reserved for stderr.

Device discovery scans `adb devices -l` for physical Android devices and running
emulators, plus `emulator -list-avds` for stopped AVDs. Set `ANDROID_EMULATOR_PATH`
to override the emulator executable; otherwise it is resolved alongside ADB,
from SDK environment variables, usual home SDK directories, or `PATH`.
Stopped AVDs appear as `avd:<name>`. Use QC **Start** or call
`mobile_boot_device` with that ID; the server launches the emulator in the
background and waits for Android boot completion. Refresh the list afterwards
and use the resulting `emulator-*` ADB ID for app/UI tools. Running AVDs are
merged with their ADB entries. Startup logs are saved to `artifacts/emulator-<name>.log`.
iOS Simulators that are shut down also have **Start** in QC.

## iOS health checks

Check an existing Simulator or physical iPhone session without starting the MCP
server, launching/building/installing a helper, recording video, or resetting a device:

```bash
device-bot --health <SIMULATOR_UDID_OR_IPHONE_ID>
```

The command writes a JSON report to stdout. Exit code `0` means every check
passed (`status: "healthy"`, `readyForNewSession: true`); a nonzero exit means
invalid arguments, a failed check (`unhealthy`), or an unconfirmed/warning check
(`degraded`). Diagnostics go to stderr. Missing helpers and unavailable probes
never count as successful checks.

Simulator probes cover CoreSimulator discovery, boot state, a guest command,
a screenshot response (temporary PNG removed after the probe), open recording files held by SimRenderServer,
XCTest project/build products, and a live helper heartbeat. Physical iPhone
probes cover CoreDevice discovery/connection and a live helper heartbeat; they
do not inspect recording ownership or application responsiveness.

The heartbeat uses the existing MCP bridge's authenticated `/health` endpoint,
which queues a helper command and waits for its result. Start a normal MCP/UI
session first if a live helper check is needed. Use the same
`DEVICE_BOT_IOS_SIM_AGENT_BIND` / `DEVICE_BOT_IOS_SIM_AGENT_TOKEN` (Simulator)
or `DEVICE_BOT_IOS_AGENT_BIND` / `DEVICE_BOT_IOS_AGENT_TOKEN` (physical iPhone)
as that session. Default bridge addresses are `127.0.0.1:18788` and
`127.0.0.1:18787` respectively. An older running bridge without `/health` needs
a session restart with the updated binary.

The bridge is shared within each platform: its heartbeat confirms a connected
helper, and does not identify which device answered when multiple helpers use
one bridge. Run with one target/helper per bridge for device-specific diagnosis.
It also does not prove the foreground app's UI/event loop is responsive.

Simulator recording startup runs the recording ownership check too. A busy or
unattributed open video file blocks startup and reports owner PID/path details;
`recordingOwners` also includes the session PID encoded in Device Bot filenames
and whether that PID exists. This is a read-only observation, not a recorder
reservation or permission to terminate a process. Inspect `recommendedActions`
and the owning session before taking recovery actions.

## QA report service

`device-bot start` launches the long-running HTML service for multiple projects,
run history, failed-step details and image/video/log downloads:

```sh
device-bot start
```

It serves `http://127.0.0.1:8765` and stores immutable result/evidence snapshots
under `~/.device-bot/reports`. Register projects and publish each completed run
through the UI. The bundled `robot-report` CLI is also available for direct
management; pass `--data-dir ~/.device-bot/reports` to use the launcher's history.
For a foreground service or QA access over LAN, use `robot-report start`
with an explicit `--data-dir` and optionally `--listen 0.0.0.0:8765`.
See [the report service guide](crates/robot-report/README.md) for commands,
project boundaries, history persistence and supported artifacts.

## YAML flows

`robot-flow` is a declarative MCP stdio client. It completes the MCP lifecycle,
validates required tools, and translates YAML steps into existing MCP calls.
It no longer constructs platform drivers. Scenario orchestration, profiles,
selectors and declarative recipes stay in the client; device routing stays in
device-bot. See [the YAML client contract](docs/yaml-mcp-client.md).

Validate the shared Orbit scenarios without starting a server or touching a device:

```bash
cargo run --manifest-path server/Cargo.toml -p robot-flow -- --dry-run \
  --profiles examples/flutter_app/automation_test/targets.yaml \
  --target ios-sim examples/flutter_app/automation_test/it-0*.yaml
```

For live execution, prepare the fixture outside the runner, pass an explicit
device and confirm the profile's setup preconditions. The linked contract lists
commands, current capability limits and migration from the old v1 browser flows.
Release 0.1.4 and later bundles device-bot, robot-flow and robot-report; the
installer installs all three CLIs together. Neither the MCP server nor its tool schemas
are changed by this YAML client implementation.

## Install a released MCP server

The release installers target the native MCP binary `device-bot` and
verify the downloaded archive against the release `checksums.txt` manifest.

macOS:

```bash
curl --fail --silent --show-error --location \
  https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.sh \
  | sh
```

Windows PowerShell:

```powershell
$script = Join-Path $env:TEMP "device-bot-install.ps1"
Invoke-WebRequest -UseBasicParsing `
  https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.ps1 `
  -OutFile $script
powershell -ExecutionPolicy Bypass -File $script
Remove-Item $script
```

Set `DEVICE_BOT_VERSION` to pin a release and `DEVICE_BOT_INSTALL_DIR` to
override the default install directory. The release archives must be named
`device-bot-v<VERSION>-<TARGET>.tar.gz` on macOS and `.zip` on Windows.
Version 0.1.1 and later ship XCTest projects in
`<install-dir>/device-bot-assets/<version>/`, so iOS automation does not require
the release builder's checkout. Run `device-bot --check-install` to validate the
installed version and complete asset manifest before starting CI. An explicit
`DEVICE_BOT_ASSET_DIR` can override the versioned asset root for development.
After installation, configure the MCP client to launch `device-bot mcp`
over stdio, or pass `--listen HOST:PORT` for the HTTP transport.
For clients with separate command and argument fields, use `command: "device-bot"`
and `args: ["mcp"]`. A bare `device-bot` opens the web app instead of serving MCP.

## Upgrade a released MCP server

The server follows my-cleaner's explicit upgrade behavior: it does not check
or replace itself in the background. `--check-update` queries the latest
GitHub release and returns update metadata only after the platform archive is
present in `checksums.txt`; `--upgrade` then reruns the verified installer for
the current installation directory.

```bash
device-bot -v
device-bot --check-update
device-bot -u
```

`-v` is `--version`, `-h` is `--help`, and `-u` is `--upgrade`.

On Windows, `--upgrade` schedules the PowerShell installer, exits the running
server, waits for that process to release the `.exe`, and then replaces it.
Development builds under `target/` refuse automatic upgrade; install a
release binary first. Network failures are reported and never alter the
current binary. Re-running either platform installer is also a supported
manual upgrade path.

## HTTP transport and auth

Run an HTTP listener explicitly; prefer a loopback address unless a reverse
proxy supplies the network boundary:

```bash
cargo run -p device-bot -- mcp --listen 127.0.0.1:3337
```

The server prints an auto-generated `DEVICE_BOT_AUTH` token to stderr and
passes it to the Flutter companion automatically. Set `DEVICE_BOT_AUTH` to
reuse a known token. After the HTTP socket is ready, a local `flutter_app/`
source tree is started with `flutter run` in the background when Flutter is
available. Set `DEVICE_BOT_FLUTTER_AUTOSTART=0` for headless or CI runs.

QC does not require manual Bearer token entry for a local server. The HTTP
server publishes an owner-private connection profile at
`~/.device-bot/mcp/connection-<port>.json`. The standalone QC desktop app runs
without App Sandbox; MCP does not access QC's old sandbox container.
QC discovers credentials only for a matching loopback URL,
scheme and port. Other MCP clients continue to use the normal Bearer header.

QC checks device readiness every five seconds using a separate read-only MCP
session. A locked iPhone is reported as `device_locked` both by health checks and
blocked UI tool calls. QC shows an unlock warning and a Check after unlocking
action; it does not replay the interrupted tool automatically. Unknown lock
state is not treated as evidence that the phone is locked.

For macOS desktop control, App Sandbox and privacy permissions are separate.
Launching apps and changing focus use AppKit without Apple Events. Screenshots
and recordings still require [Screen Recording permission](https://support.apple.com/en-gb/guide/mac-help/mchl592e5686/mac),
and inspecting or controlling other apps requires [Accessibility permission](https://support.apple.com/en-mn/guide/mac-help/mh43185/mac).
Keyboard input still uses System Events and may require [Automation permission](https://support.apple.com/en-nz/guide/mac-help/mchl108e1718/mac).
Removing App Sandbox does not grant these permissions automatically.

`POST /mcp` implements the `2025-11-25` session lifecycle. Send the token as
`Authorization: Bearer <token>`. An
`initialize` response returns `Mcp-Session-Id`; repeat it for subsequent calls,
send `notifications/initialized`, then call tools. Responses are JSON by
default, or one SSE `message` event when the client sends
`Accept: text/event-stream`. `DELETE /mcp` with the same session header closes
the in-memory session.

## Optional local admin API

The same explicit HTTP listener also exposes local operator endpoints:

| Endpoint | Purpose |
| --- | --- |
| `GET /admin/devices` | Current devices from the robot registry |
| `GET /admin/devices/{device_id}/screenshot` | Current device PNG for the companion viewer |
| `GET /admin/devices/{device_id}/elements` | Normalized accessibility tree used to draw labeled bounds over the screenshot; supports optional `filter` and `maxResults` query parameters |
| `GET /admin/config` | Safe-to-display auth and log-buffer status |
| `GET /admin/logs` | Bounded, recent MCP request activity |
| `GET /admin/logs/stream` | Future activity as server-sent events |

They share the HTTP listener's bearer-token policy. The Flutter companion
source lives in [`flutter_app/`](flutter_app/), but uses the MCP session above
for device discovery, boot, app launch, screenshot, accessibility inspection,
and manual actions. These admin endpoints remain available for diagnostics and
are not part of the companion's device-control path. The companion is a client
only; the Rust CLI owns the server lifecycle and launches the companion after
the MCP listener is ready.

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
| Windows host | screenshot, launch app, tap, text, drag, UIA accessibility elements, opt-in Flutter widget-tree discovery | No recording, installer/uninstaller, or OmniParser coordinate fallback. Requires an interactive Windows session; target and MCP process elevation/desktop session must be compatible. |
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

When querying a Flutter Web page, the Chrome driver automatically activates
Flutter's standard **Enable accessibility** placeholder and waits up to 1.5 seconds
for the semantic DOM. This runs inside MCP on each query, including after
navigation, so CI does not need a separate CDP script or manual click. Queries on
ordinary HTML pages and already-enabled Flutter pages do not click anything.
An activation error or timeout is reported as a driver error, rather than a
successful empty tree.

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
| `mobile_press_ime_action` | `{ "deviceId": "emulator-5554", "action": "next" }` | Presses `next`, `done`, `go`, `search`, or `send` on the focused editor. Android sends KEYCODE_ENTER only when `dumpsys input_method` reports that same action |
| `mobile_take_screenshot` | `{ "deviceId": "emulator-5554" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_click_on_screen_at_coordinates` | `{ "deviceId": "emulator-5554", "x": 100, "y": 240 }` | Text confirmation |
| `mobile_swipe_on_screen` | `{ "deviceId": "emulator-5554", "startX": 100, "startY": 800, "endX": 100, "endY": 200 }` | Swipe confirmation; optional `durationMs` |
| `mobile_list_elements_on_screen` | `{ "deviceId": "emulator-5554", "elementType": "textField", "filter": "Save", "maxResults": 20 }` | `structuredContent.elements`. `elementType` and `filter` are AND conditions applied before `maxResults`. `textField` includes password fields. Omitted `focused`, `checked`, or `selected` means unknown |

## iOS Simulator tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `mobile_take_screenshot` | `{ "deviceId": "iOS-simulator-UDID" }` | MCP `image` content (`image/png`, Base64) |
| `mobile_click_on_screen_at_coordinates` | `{ "deviceId": "iOS-simulator-UDID", "x": 100, "y": 240 }` | Bundled XCTest touch at the screen point |
| `mobile_type_keys` | `{ "deviceId": "iOS-simulator-UDID", "text": "hello" }` | Bundled XCTest keyboard input |
| `mobile_press_ime_action` | `{ "deviceId": "iOS-simulator-UDID", "action": "done" }` | Taps the keyboard key with that exact label. Rebuild the XCTest runner after upgrading the server |
| `mobile_swipe_on_screen` | `{ "deviceId": "iOS-simulator-UDID", "startX": 100, "startY": 700, "endX": 100, "endY": 200 }` | Bundled XCTest touch swipe; optional `durationMs` |
| `mobile_list_elements_on_screen` | `{ "deviceId": "iOS-simulator-UDID", "elementType": "textField", "filter": "Baby Shark", "maxResults": 10 }` | XCTest selects the element-type query, then applies the text filter and `maxResults`. Rebuild the simulator runner before using `elementType` |
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

Real-device helper builds automatically run `configure_signing.py` before
computing the build cache identity, including Prepare / Install in the QC app.
The script uses valid development certificates in Keychain, reads the Team ID
from the certificate's `OU`, and replaces every `DEVELOPMENT_TEAM` setting in
the helper project. It requires `python3`, `security`, and `openssl`.

Run `server/tool/configure_ios_signing.sh` to synchronize signing separately;
add `--print-only` to inspect the team, or `--project path/to/App.xcodeproj` to
update another project. With multiple installed teams, a matching project team
is kept; otherwise select `DEVICE_BOT_IOS_TEAM`. Missing or ambiguous identities
leave the project unchanged, and matching settings are not rewritten.

The Rust entry point is `device-bot --configure-ios-signing`; it runs the same
shell script and returns the selected Team ID as JSON without starting a
server. Prepare / Install in QC invokes this signing setup automatically before
building. If iOS installs the helper but refuses to launch it until its developer
certificate is trusted, QC shows a persistent Trust warning with the iPhone
Settings path and a Retry after Trust action.

During helper startup, Install activates the XCTest runner itself and waits for
it to enter the foreground before making the first local-network request. On
first use, allow the iOS Local Network dialog on the iPhone. The runner waits for
that choice and only enters its command loop after network access is available.

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
it starts this repository's `device-bot`, binds the agent bridge to a
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

1. Configure Codex to start this repository's `device-bot` with
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
Windows also lacks recording and the planned OmniParser coordinate fallback.
For Flutter Windows apps, call `await DeviceBotFlutter.start()` from
[`packages/device_bot_flutter`](../packages/device_bot_flutter/README.md) before `runApp`.
Native accessibility is queried first. The Flutter widget tree is consulted
only when the selector has no native matches; ambiguity and native errors remain
visible. The bridge also supports macOS, Android/iOS device endpoints, and web;
see the package guide for platform setup. Configured IME actions
and vertical touch swipes are available through the Flutter bridge. Literal
text uses Win32 Unicode input, and the target foreground PID is verified before
input. The sample in
`examples/flutter_app` includes this startup call. Launch the app through
`mobile_launch_app` to retain its PID as the automation target.

Chrome pages are exposed when a DevTools endpoint is available. If it has no
controllable page, discovery returns an offline `chrome:browser` row. Choose
QC **Start** or call `mobile_boot_device` with `chrome:browser` to open a local
Chrome automation session, then refresh to select a `chrome:<target-id>` page.
The server uses a separate profile under `~/.device-bot/chrome/<port>` and leaves
Chrome running. Set `DEVICE_BOT_CHROME_PATH` to override its executable.
Only loopback DevTools endpoints can be started automatically.
You can also start Chrome manually with a separate profile, for example:

```sh
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/device-bot-chrome
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
cargo run -p device-bot -- --install-chrome-recording-tools
```

You can request the macOS permission before starting the server:

```sh
cargo run -p device-bot -- --request-screen-recording-permission
```

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

# Device Bot

Automate Android, iOS, desktop apps, and Chrome through a native Rust MCP
server. Use your MCP client, control a device in the QC app, or replay YAML
scenarios and review their evidence in the report web app.

[Install](#install) · [Quick start](#quick-start) · [MCP setup](#connect-an-mcp-client) ·
[Platforms](#supported-platforms) · [Live control](#live-control) · [YAML](#run-yaml-scenarios) ·
[Documentation](#documentation)

## What you can do

- Discover devices and apps, inspect UI elements, and send taps, text, and gestures.
- Watch screen video in QC Live while inspecting and controlling the selected target.
- Record supported QC interactions as YAML, then replay flows through the same MCP tools.
- Keep run history, step results, screenshots, and recordings in persistent QA reports.

The platform table describes the current source. See the [changelog](CHANGELOG.md)
for the features included in your installed release; capabilities vary by target.

## Install

Native release bundles are available for **macOS Apple Silicon**, **macOS Intel**,
and **Windows x64**. The installers download the latest published release and
verify its archive against the release checksums. Rust and Flutter are not
required on the receiving machine.

### macOS

```sh
curl --fail --silent --show-error --location \
  https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.sh \
  | sh
```

The installer uses an existing FFmpeg with `libx264`, or installs `ffmpeg-full`
through Homebrew. Install [Homebrew](https://brew.sh/) first if neither is available.

### Windows PowerShell

```powershell
$installerPath = Join-Path $env:TEMP "device-bot-install.ps1"
try {
  Invoke-WebRequest -UseBasicParsing -ErrorAction Stop `
    https://raw.githubusercontent.com/ho-doan/device-bot/master/scripts/install.ps1 `
    -OutFile $installerPath
  powershell -NoProfile -ExecutionPolicy Bypass -File $installerPath
  if ($LASTEXITCODE -ne 0) { throw "Device Bot installer exited with status $LASTEXITCODE." }
} finally {
  if (Test-Path -LiteralPath $installerPath -PathType Leaf) {
    Remove-Item -LiteralPath $installerPath -Force -ErrorAction Stop
  }
}
```

Open a new Windows terminal after installation so the PATH change takes effect.
On macOS, add the install directory printed by the installer to PATH if needed.
Both installers include the CLI tools, QC desktop/web assets, and matching
scrcpy/ADB resources for Android Live. Windows also gets portable FFmpeg.

See the [installation and upgrade guide](server/README.md#install-a-released-mcp-server)
for pinned versions, custom directories, asset checks, and manual downloads from
[GitHub Releases](https://github.com/ho-doan/device-bot/releases).

## Quick start

Verify the installation, then open the QC desktop app:

```sh
device-bot --check-install
device-bot qc
```

QC starts the local MCP backend when needed. Connect or boot a target, launch
its app or open a Chrome URL, then inspect the UI or open **Live**. Each platform
still needs the setup listed below.

| Command | Use |
| --- | --- |
| `device-bot` | Open the report web app; start its background service if needed |
| `device-bot start` | Ensure the report service is running |
| `device-bot qc` | Open QC desktop and ensure local MCP is running |
| `device-bot mcp` | Serve MCP over stdio |
| `device-bot run scenario.yaml` | Run a YAML scenario |
| `device-bot -v` / `device-bot -h` | Show the version / command help |
| `device-bot --check-update` / `device-bot -u` | Check for / install a verified update |

The report service survives closing the terminal. Installed builds store history
under `~/.device-bot/reports`; `DEVICE_BOT_REPORT_DATA_DIR` overrides that location.

## Connect an MCP client

Configure the client to run **`device-bot mcp`**. For clients with an
`mcpServers` JSON configuration:

```json
{
  "mcpServers": {
    "device-bot": {
      "command": "device-bot",
      "args": ["mcp"]
    }
  }
}
```

Use the installed executable's absolute path if the client cannot find it on
PATH. A bare `device-bot` opens reports; the `mcp` argument selects the server.

For an HTTP MCP client or QC in a browser:

```sh
device-bot mcp --listen 0.0.0.0:3337
```

The MCP endpoint is `/mcp`. Open `http://127.0.0.1:3337/qc/` locally, or
`http://<server-ip>:3337/qc/` from the LAN, and enter the backend's bearer token.
The desktop app discovers its local token automatically. Devices and drivers
run on the backend machine. See [HTTP transport and auth](server/README.md#http-transport-and-auth).

Example requests for an agent: “List available devices”, “Inspect the login
screen on this device”, or “Enter the demo credentials and verify the workspace
heading”. The server exposes device, app, screen, input, UI-tree, Live-stream,
and flow-execution tool families; the [server guide](server/README.md#device-tools)
documents their schemas and platform limits.

## Supported platforms

| Target | Main capabilities | Required setup / limits |
| --- | --- | --- |
| Android device or emulator | Native elements, app management, input, screenshots, recording, Live | Authorized ADB connection or a booted emulator; bundled scrcpy powers Live |
| iOS Simulator | Boot, app management, XCTest elements/input, screenshots, recording, Live | Xcode on a Mac and the bundled XCTest runner |
| Physical iPhone | XCTest elements/input, app management, screenshots, USB or Wi-Fi Live | Paired iPhone, Developer Mode, signed helper, reachable Mac bridge; Wi-Fi sharing needs **Start Broadcast** |
| macOS app/window | Accessibility elements, app launch, native input, screenshots, recording, Live | Accessibility and Screen Recording permissions for the relevant operations |
| Windows app/window | UI Automation elements, app launch, input/drag, screenshots, Live | Interactive desktop with compatible elevation; app installation and screen-recording tools are not implemented |
| Chrome page | DOM elements, navigation, input, screenshots, recording, Live | Chrome with a DevTools session; QC **Start** can prepare a separate browser profile automatically |

Browser-page automation uses the Chrome target (`chrome:…`). Launching a browser
through an operating-system driver does not provide its DOM. Full operation
details are in the [platform matrix](server/README.md#platform-support-and-known-gaps).

## Live control

QC Live combines video, element inspection, and explicit input actions. Select
an element to inspect it; use **Tap selected**, text controls, or gestures to
control the app. Recorded supported actions can be reviewed and exported as YAML.

Chrome prefers browser-native capture and uses CDP screencast plus FFmpeg when
that capture path is unavailable. Native browser sharing prompts still apply.
See [web Live setup and limitations](server/README.md#web-recovery-and-live).

### iPhone over Wi-Fi

Follow the [iPhone Wi-Fi setup](server/README.md#live-video-over-wi-fi-on-a-real-iphone)
to configure signing and a LAN-reachable bridge. Opening Live prepares the
bundled **Device Bot Broadcast** app. On the iPhone, allow Local Network, tap
the record button, and choose **Start Broadcast**. Screen video then streams
through ReplayKit, H.264, and WebRTC; audio is not sent.

Closing Live stops sharing. Reopening it requires a new broadcast confirmation;
reopen after rotation or a lost connection. The separate QuickTime MP4 recording
tool still requires USB. [Physical-device verification](docs/live-ios-wifi-verification-2026-11-14.md)
records the tested scope; this is not a promise of full platform feature parity.

## Run YAML scenarios

Save this as `login.yaml`. It expects the [Orbit Flutter fixture](examples/flutter_app/README.md)
installed on Android; replace `emulator-5554` with your device ID.

```yaml
version: 2
name: orbit_login
steps:
  - select_device: {id: emulator-5554}
  - launch_app: {app_id: com.example.flutter_app}
  - assert_element: {label: "Welcome back."}
  - tap_element: {label: "Try demo workspace"}
  - assert_element: {label: "A fresh start, Alex."}
  - screenshot: {path: workspace.png}
```

```sh
device-bot run --dry-run login.yaml   # Validate without touching a device
device-bot run login.yaml            # Execute and retain run evidence
```

`device-bot run` uses the bundled `robot-flow` client and the same MCP tools as
QC. For reusable selectors, target profiles, assertions, timeouts, and report
artifacts, read the [YAML guide](server/docs/yaml-mcp-client.md). Prepared native
control examples are in [Orbit control smoke](examples/automation_test/README.md);
the [shared Flutter scenarios](examples/flutter_app/automation_test/README.md)
document their verified and partial coverage.

## Documentation

- [Server guide](server/README.md): configuration, tools, platform setup, troubleshooting.
- [QC desktop setup](server/README.md#qc-desktop-app): build or locate the companion app.
- [Report service](server/crates/robot-report/README.md): projects, history, results, evidence downloads.
- [Flutter discovery bridge](packages/device_bot_flutter/README.md): optional widget discovery for target apps.
- [Architecture](server/docs/architecture.md) and [Live architecture](docs/architecture-live.md).
- [Examples](examples): Flutter, SwiftUI, Android Compose, React Native, Windows, Vite, and Electron.
- [Changelog](CHANGELOG.md).

## Build and verify from source

Build all three native tools from the repository root:

```sh
cargo build --manifest-path server/Cargo.toml --release --locked \
  -p device-bot -p robot-flow -p robot-report
server/target/release/device-bot mcp
```

Building QC desktop itself also requires Flutter and the host platform build
tools; follow the [QC build instructions](server/README.md#qc-desktop-app).
Debug builds use separate development ports and data directories; see `-h`.

```sh
cargo test --manifest-path server/Cargo.toml --workspace --locked
node --test server/crates/robot-chrome/tests/dom-elements.test.cjs
node --test server/crates/robot-chrome/tests/live.test.cjs
python3 scripts/test_package_release.py
```

The [fixture app](examples/flutter_app/README.md),
[companion](server/flutter_app/README.md), and
[Flutter bridge](packages/device_bot_flutter/README.md) have their own Flutter
checks. Core code lives in `server/crates/`, QC in `server/flutter_app/`, and
release tooling in `scripts/`.

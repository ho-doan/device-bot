# Repository map

Start with the [end-to-end flow in the README](../README.md#how-the-repo-fits-together).
This map connects that flow to the applications and source code in this repo.

## Product applications

| App / tool | Technology | Role in the flow | Source |
| --- | --- | --- | --- |
| `device-bot` | Rust, Tokio, Axum, MCP / JSON-RPC | CLI entry point; starts QC or reports; serves device tools over stdio or HTTP | [mcp-server](../server/crates/mcp-server) |
| QC Companion | Flutter / Dart, HTTP, `flutter_webrtc` | Desktop/web device selection, element inspector, Live control, YAML review/export and replay | [QC app](../server/flutter_app) |
| `robot-flow` | Rust, YAML v2, MCP client | Replays scenarios, applies target profiles and writes per-step results/evidence | [flow runner](../server/crates/robot-flow), [YAML contract](../server/docs/yaml-mcp-client.md) |
| `robot-report` + QA Reports | Rust / Axum; HTML, CSS, JavaScript | Imports and retains project/run snapshots; browser history, filters and evidence downloads | [report app](../server/crates/robot-report) |
| Device Bot Broadcast | Swift / SwiftUI, ReplayKit, VideoToolbox | iPhone Wi-Fi screen capture; the user starts a system broadcast; H.264 enters the existing Live stream | [broadcast app + extension](../server/crates/robot-ios-real/xctest-runner/WifiBroadcast) |

QC and Reports are separate interfaces. QC controls targets; Reports reviews
saved runs. `robot-report` can serve its own site; the MCP HTTP server can also
mount reports at `/reports/` alongside QC web at `/qc/`. Stdio is the MCP-only
entry point. Published native bundles target macOS and Windows; this does not
mean every driver or tool supports every host OS.

## Platform adapters and helpers

| Target | Control / discovery | Live / capture path | Source |
| --- | --- | --- | --- |
| Android | ADB, native UI hierarchy, device/app commands | scrcpy / MediaCodec H.264 | [robot-android](../server/crates/robot-android) |
| iOS Simulator | `simctl`, bundled XCTest UI runner | Simulator capture and encoding through the native driver | [robot-ios-sim](../server/crates/robot-ios-sim) |
| Physical iPhone | go-ios and signed XCTest helper; optional WebView inspection helper | USB AVFoundation / VideoToolbox, or Wi-Fi ReplayKit broadcast | [robot-ios-real](../server/crates/robot-ios-real) |
| macOS app/window | Accessibility, native input | ScreenCaptureKit / VideoToolbox | [robot-macos](../server/crates/robot-macos) |
| Windows app/window | UI Automation, native input | Windows Graphics Capture / Media Foundation | [robot-windows](../server/crates/robot-windows) |
| Chrome page | Chrome DevTools Protocol, DOM and frame contexts | Browser-native capture; CDP screencast + FFmpeg fallback where supported | [robot-chrome](../server/crates/robot-chrome) |
| Flutter target app | Optional native/web widget discovery bridge | Supplements inspection; the platform driver still owns device control/capture | [device_bot_flutter](../packages/device_bot_flutter), [robot-flutter-bridge](../server/crates/robot-flutter-bridge) |

`robot-core` defines the platform-neutral device contracts. `mcp-protocol`
defines wire types, `admin-api` serves operator read models, and `robot-stream`
shares stream/session infrastructure. Platform setup, signing, permissions and
known gaps are in the [platform matrix](../server/README.md#platform-support-and-known-gaps).
The `uia_windows` Dart package is a compatibility export for the Flutter bridge.

## Example apps: one behavior, several frameworks

These are **automation targets**, not production Device Bot clients. They
implement the Orbit login/workspace behavior so selectors, input, keyboard and
navigation can be exercised across stacks. Their amber icon distinguishes them
from the teal Device Bot tools. Sharing behavior does not imply equal test
coverage or identical native element trees.

| App | Framework / languages | Target environments | Automation entry point |
| --- | --- | --- | --- |
| [Flutter Orbit](../examples/flutter_app) | Flutter / Dart | Android, iOS, macOS, Windows, web | [Flutter scenarios](../examples/flutter_app/automation_test); native [control smoke](../examples/automation_test) |
| [Android Orbit](../examples/android-app) | Kotlin / Jetpack Compose | Android | `targets-compose.yaml` with shared control smoke |
| [Swift Orbit](../examples/swift-app) | Swift / SwiftUI | Xcode native Apple targets | Behavior fixture; inspect the selected build before authoring selectors |
| [React Native Orbit](../examples/rnApp) | React Native / TypeScript | Android, iOS | `targets-react-native.yaml` and its control-flow variant |
| [WPF Orbit](../examples/win-app) | C# / .NET / WPF | Windows | `targets-wpf.yaml` candidate profile; native Windows validation is recorded separately |
| [Web Orbit](../examples/vite-app) | React / TypeScript / Vite | Chrome page | Chrome driver + observed DOM selectors |
| [Electron Orbit](../examples/elec-app) | Electron / Vue / TypeScript | Desktop Chromium app | Native desktop target or a configured Chrome DevTools session |

The shared behavior source is
[BEHAVIOR_FLOWS.md](../examples/flutter_app/BEHAVIOR_FLOWS.md). Profiles and
coverage notes live in [examples/automation_test](../examples/automation_test).

## From implementation to a repeatable flow

1. **Implement and build** the target app. Keep the acceptance criteria and
   initial state/data available; select the correct platform driver.
2. **Inspect and draft** with the QC inspector/Live recorder or an agent using
   [implement-to-mcp-test](../implement-to-mcp-test/SKILL.md). For Flutter, start
   with QC-style selectors from a real snapshot: a unique exposed ID, a unique
   label, or a scoped position when necessary. Run short segments before
   optimizing selectors.
3. **Validate and replay** the YAML with `device-bot run --dry-run flow.yaml`,
   then `device-bot run flow.yaml` against the prepared target. Dry-run checks
   configuration; live execution proves the actions were attempted on that build.
4. **Review evidence** in QA Reports. Inspect screenshots/video and control
   transitions; runner PASS alone is not proof of all intended behavior. Add
   assertions when automated verification is part of the requested scope.
5. **Run in CI** where the target environment exists. The
   [QC automation workflow](../.github/workflows/automation-tests.yml) builds QC
   web, tests the harness/drivers, and replays its web YAML suite on Linux and
   Windows. It does not validate every example app or physical-device path.

## Build and delivery

Rust/Cargo builds the three native tools. Flutter builds QC desktop and the
web assets. Python scripts package matching tools/assets and test installers;
GitHub Actions runs QC automation, warms release caches, and builds native
release archives. The [release workflow](../.github/workflows/release.yml)
uses the QC automation workflow as a gate before release jobs.

Each native archive includes `LICENSE` and `NOTICE`; installers retain these
under `licenses/device-bot/` beside the installed tools. Dependencies such as
scrcpy, FFmpeg and Node packages retain their upstream licensing. Icons are
generated from [one editable SVG source](../assets/brand/README.md).

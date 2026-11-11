## Unreleased

## 0.1.9 — 2026-11-11

* Fix Windows compilation of the shared screenshot module by qualifying its
  timeout type independently of macOS-only imports.
* Reissue the native tools after the 0.1.8 release workflow failed before publishing.

## 0.1.8 — 2026-11-11

* Add Orbit examples for Android Compose, SwiftUI, React Native, Vite React,
  and Electron Vue, with a shared YAML smoke flow for native/CDP taps and text input.
* Discover stopped Android emulators and Chrome sessions, boot them from device
  control, and retain device lock ownership during local connection discovery.
* Add desktop display selection and app-scoped screenshots, iOS signing
  configuration, and per-step platform routing in YAML flows.
* Improve Flutter field activation, positional controls, keyboard dismissal,
  and independent prefix/suffix actions.
* Update the MCP lifecycle regression to cover the display-listing tool.

## 0.1.7 — 2026-11-09

* Rename the Flutter bridge to device_bot_flutter, retaining the uia_windows
  compatibility API. Add web/CDP, macOS geometry, and authenticated device
  endpoints for Android/iOS widget discovery.
* Keep native accessibility first and query Flutter only for a missing selector;
  preserve native ambiguity/errors and perform taps and typing via native input.

## 0.1.6 — 2026-11-09

* Finish Chrome target discovery after reading Content-Length bytes, recognize
  Flutter semantics buttons as clickable, and preserve empty field values.
* Resolve selector profile variables before YAML execution and poll until the
  declared timeout instead of stopping after 12 attempts.

* Remove the legacy root Flutter controller, its Windows Pigeon/C++ plugin,
  vision sidecar, tests, and obsolete editor configuration. Native platform
  drivers, the optional Flutter companion, and `packages/uia_windows` remain.

## 0.1.5 — 2026-11-06

* Use `device-bot mcp` for the MCP server and `device-bot run <scenario.yaml>` for YAML execution.
* Make bare `device-bot` start the background report web app and open the browser; `device-bot start` reuses the healthy service or starts it when absent.
* Keep the web app running after the terminal closes, with history in `~/.device-bot/reports` and default port 8765.
* MCP clients that previously launched bare `device-bot` must add the `mcp` argument.

## 0.1.4 — Native tools

* Release device-bot, robot-flow and robot-report together for macOS arm64/x86_64 and Windows x86_64.
* Add persistent multi-project QA HTML reports with run history, failed-step details and original evidence downloads.
* Include production systemd/LaunchDaemon templates and reporter health/version commands.
* Ship YAML-to-MCP flows and corrected Flutter Semantics profiles; Simulator regression matches the shell baseline (4 PASS, 5 PARTIAL).
* Add bounded iOS health probes and Simulator recording ownership preflight.
* Verify all three CLI versions during installation while retaining support for older device-bot-only archives.

## 0.1.1

* Package versioned iOS Simulator and real-device XCTest assets with native releases.
* Resolve installed assets relative to the executable; add `--check-install`.
* Preserve ephemeral bridge ports when launching XCTest agents.
* Install and verify binary/assets together for reproducible CI setup.

## 0.0.1

* TODO: Describe initial release.

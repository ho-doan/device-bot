## 0.1.25 — 2026-11-25

* Fix real-iPhone Live over USB by waiting for AVFoundation to register the
  phone's screen and keeping one capture session for geometry and video.
  Document how to map a capture ID that differs from the Xcode UDID.
* Add YAML projects with named device and browser resources, fixture hooks,
  staged artifacts, and scoped runs that can reuse sessions across cases.
* Add read-only device readiness reports and an MCP tool to stop simulator apps.
  Improve recording isolation, playback deadlines, and evidence validation.

## 0.1.24 — 2026-11-16

* Add an Enter button to macOS QC Live. Send a real Return key event for
  standalone newline input so focused editors submit instead of inserting a newline.
* Cover literal text preservation in QC and native Return submission with an
  AppKit fixture regression test.

## 0.1.23 — 2026-11-16

* Add opt-in Windows MSAA/IAccessible discovery for native queries and Live,
  selectable with `DEVICE_BOT_WINDOWS_ACCESSIBILITY=msaa`. Keep UIA as the default.
* Add native-only accessibility snapshots and a release-app validation guide to
  evaluate Flutter release automation without an embedded widget-tree bridge.
* Preserve empty native editor values. Bound MSAA traversal, validate window
  ownership, exclude hidden controls and withhold protected text.

## 0.1.22 — 2026-11-16

* Preserve literal text, including YAML-sensitive strings, through Live recording,
  inspector export and flow replay. Add recording contract checks to automation CI.
* Organize QC features around Cubits, Dio and generated models, with typed JSON
  boundaries and clearer dialog and Live state management.
* Separate CLI parsing and startup, core contracts and driver routing, flow
  validation and execution, and macOS native bindings. Preserve typed flow
  failures through execution and cleanup, and remove platform build warnings.
* Configure fixture downloads before disposable browser pages run. Expand
  regression coverage for lost acknowledgements, invalid input, authentication,
  frame authorization and concurrent report history.

## 0.1.21 — 2026-11-15

* Add the Apache-2.0 license and package metadata. Include LICENSE and NOTICE
  in release archives and retain them under `licenses/device-bot/` on install.
* Introduce a shared Device Bot icon for QC, QA Reports and the iPhone Broadcast
  app, with an amber variant for Orbit example apps and a reproducible SVG-to-icon tool.
* Add the repository technology/app map and an end-to-end Mermaid workflow
  connecting agents, QC, YAML replay, platform drivers, Live and report review.
* Run the QC web automation suite on Linux and Windows before publishing.
  Bring report controls into view on short windows and exclude locked Chrome
  profiles from evidence uploads. Configure fixture downloads for each flow's
  browser context so an inactive page cannot prevent report download checks.
* Teach `implement-to-mcp-test` to draft Flutter YAML like the QC inspector:
  use observed IDs/labels or scoped positions, run a stable flow first, then
  optimize selectors when useful.
* Skip Rust cache warming on exact cache hits and retain build timings for misses.

## 0.1.20 — 2026-11-15

* Align Live video, element overlays and pointer mapping with the decoded frame
  when its aspect ratio differs from the driver's input coordinate space. Keep
  alignment when QC is narrowed or resized, and clip boxes to the video area.
* End Chrome fallback capture when the target viewport or zoom changes, so Live
  reconnects with fresh coordinates instead of using the previous window size.
* Disable **Tap selected** while its click is pending, including same-frame
  duplicate clicks, and unlock it after success or failure. Queue Chrome mouse
  release before awaiting press acknowledgements to avoid native popup stalls.
* Retry transient Live inspector failures independently of healthy video, with
  bounded retries and a manual retry action. Ignore stale app/window picker
  responses when switching targets quickly.
* Allow bounded extra startup time for Windows UI Automation helpers, including
  PowerShell and UIA assembly loading, instead of failing cold starts after 2.5s.
* Make Flutter web popup menus selectable through Chrome automation and respect
  disabled menu items. Add repeatable Windows-backed web Live regression tests.
* Disable host-cursor drawing in Windows Live's FFmpeg capture to avoid cursor
  capture interference on Windows/RDP. This does not change Windows mouse
  settings or desktop input, and is separate from hiding helper consoles.
* Bind Windows Live tracks through an owned native stream and retain encoder
  failure diagnostics. Allow QC desktop to connect to remote servers with an
  explicit bearer token, including pasting from the clipboard.
* Keep Windows Android discovery and media helpers from opening consoles, and
  include the matching web assets in every native release bundle.
* Share one Rust release version across all three CLI tools and check manifest
  and built-binary versions before compiling QC, preventing mixed-version bundles.

## 0.1.18 — 2026-11-14

* Fix the Windows installer reporting `Remove-Item` errors after a successful
  install when the temporary bootstrap script was already deleted. Leave
  caller-owned scripts to the launcher, guard cleanup with a literal-path
  existence check, and preserve installer failure exit codes.
* Stream a real iPhone over Wi-Fi with the bundled Device Bot Broadcast app:
  ReplayKit captures screen video, VideoToolbox encodes H.264 on the phone,
  and the existing WebRTC viewer receives it without a USB video source.
  Allow Local Network and choose **Start Broadcast** on the iPhone for each
  sharing session; audio is not streamed.
* Select iPhone Live transport with `DEVICE_BOT_IOS_LIVE_TRANSPORT` (`auto`,
  `wifi`, or `usb`). Scope Wi-Fi upload credentials to each device session,
  validate capture geometry before input, and stop sharing when Live closes.
* Keep QC Live input edits intact, refresh device discovery after opening
  Chrome, and show the current imported flow's run result and report evidence.
* Preserve Windows window titles through UTF-8 PowerShell output, reject stale
  desktop input targets, and exclude hidden DOM controls from web selectors.
* Fix iOS Simulator XCTest gesture compilation and screen-region discovery,
  and add opt-in Live checks for H.264 decode, native input, and stream reopen
  across macOS, Chrome, Android, iOS Simulator, and physical iPhone Wi-Fi.

## 0.1.17 — 2026-11-13

* Keep Windows Live app/window discovery usable when a window title contains
  malformed UTF-16, while preserving valid emoji and window identities.
* Bind iframe/WebView activation and scrolling to the selected DOM document and
  node; reject stale references instead of sending native input to old bounds.
* Ship the optional iOS WebView helper, dependency lockfile and setup guide in
  native archives and preserve it during installation.
* Read Chrome iframe documents, including nested and cross-origin frames, and
  keep their stable `web_context` on elements, input, and YAML replay. Unscoped
  selectors stay on the root document.
* Add an iOS in-app WebView helper seam. An explicit web context never falls
  back to XCTest, and a missing helper leaves native accessibility usable.

## 0.1.11 — 2026-11-12

* Replace an older local MCP server from `device-bot qc` when its `/health/live`
  version does not match this build, so QC can call `mobile_run_flow`.
* Import an exported flow YAML in the QC app and run it on the device.
* Install the QC desktop app next to the CLI from the macOS and Windows release
  archives.
* Publish Windows release checksums as `text/plain`, and read checksum bytes in
  the Windows upgrader, so `x86_64-pc-windows-msvc` upgrades can verify.

## 0.1.10 — 2026-11-12

* Add `device-bot qc` to open the optional QC desktop app against the local MCP
  server, plus a macOS packaging script and an unauthenticated `/health/live` check.
* Unify YAML launch on `launch_app` for HTTP(S) URLs and native app identifiers.
* Burn step captions and optional notes into run recordings, append a paginated
  video summary, and keep recording through sensitive inputs without writing
  secret values into evidence.
* Start each Chrome MCP session in a fresh private profile with isolated client
  state, and run YAML flows over HTTP MCP.
* Accept short flags `-v`, `-h`, and `-u` for version, help, and upgrade.
* Add a WPF Orbit example and physical-device smoke flows for SwiftUI, Compose
  test tags, and React Native USB input.

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

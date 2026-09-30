# Platform parity decision

## Decision

During phased delivery, Android and iOS may temporarily expose different
capabilities while their platform drivers are built independently. Do not
declare the product complete until a dedicated platform-parity pass has made
the shared MCP surface behave consistently on both platforms.

## Current known gap

`mobile_start_screen_recording` and `mobile_stop_screen_recording` are backed
by `xcrun simctl io recordVideo` for iOS Simulator. The Android driver does
not yet implement the equivalent `adb shell screenrecord` lifecycle and
therefore returns the portable `Unsupported` result for these operations.

The real iOS driver currently exposes `go-ios` install/launch and UI-runner
control, but leaves uninstall explicitly unsupported because `go-ios` has no
stable uninstall command in its current CLI surface. The parity pass must
resolve this shared app-lifecycle contract.

## Desktop parity status

The macOS and Windows host drivers now expose the same normalized accessibility
tree contract through their native APIs: the macOS driver traverses AXUIElement
children and the Windows driver traverses the UI Automation raw view. Both
drivers apply a case-insensitive filter while traversing and stop at
`maxResults`, so `mobile_list_elements_on_screen` does not need to materialize
an unbounded desktop tree.

Windows UIA follows the Flutter plugin's existing semantics: process-scoped
root resolution, control-type role mapping, automation id/name/help text,
physical-pixel bounds, enabled state, clickable inference, and bounded raw-view
traversal. It falls back to the foreground process only when no app has been
launched through the Windows robot. UIA remains unavailable across elevated or
different desktop sessions.

The desktop drivers intentionally retain their platform-specific gaps: macOS
supports screen recording while Windows does not; neither driver installs or
uninstalls apps; and macOS does not expose drag/swipe through its current native
backend while Windows does.

## Required parity pass

After the planned feature phases are implemented, before project completion:

1. Inventory every `Robot` capability and each MCP tool on Android and iOS.
2. Implement missing platform equivalents where feasible; otherwise make
   capability availability explicit in tool results and documentation.
3. Align argument validation, result schemas, artifact locations, lifecycle
   semantics, and actionable error messages.
4. Run the same MCP end-to-end scenarios on an Android emulator and iOS
   Simulator, saving screenshots or recordings under `artifacts/` as evidence.
5. Add regression tests for every agreed cross-platform behavior.

The parity pass owns synchronization work; individual phases remain focused on
their platform-specific driver and must not accumulate cross-platform policy in
tool router files.

# uia_windows

Temporary widget-tree discovery for Flutter Windows apps while native Windows
UI Automation does not expose their controls reliably. All Flutter traversal,
transport, registration and Win32 coordinate conversion live in `lib/`.

Add a path dependency and call start before runApp:

```yaml
dependencies:
  uia_windows:
    path: ../../packages/uia_windows
```

```dart
import 'package:flutter/material.dart';
import 'package:uia_windows/uia_windows.dart';

Future<void> main() async {
  await UiaWindows.start();
  runApp(const MyApp());
}
```

`start()` initializes the binding and is idempotent. It is a no-op on other
platforms, including web. `await UiaWindows.stop()` closes the bridge and removes
its registration; a later start registers it again. Startup errors propagate to
the caller. No app wrapper, debug service, native plugin, or fixed port is needed.

The package inspects the live Element/render tree using release-compatible
Flutter APIs. It exports common Material buttons, text fields, checkboxes,
switches, sliders, list tiles, text, images, scroll views, tappable gesture/ink
widgets, and controls with explicit button/checked/toggled Semantics (including
radio controls). ValueKey<String> and Semantics identifiers become resource IDs;
TextFormField keys are retained on their editors. Labels, enabled/focus/selection
and checked state are included where available. Obscured editor values are omitted.

Offstage, transparent, clipped and center-occluded controls are excluded. Each
query waits for layout and captures current widget state. Bounds use physical
desktop pixels with the Flutter client origin and current view DPI, including
negative monitor coordinates. The existing Windows robot still performs native
tap, typing and horizontal drag operations. Vertical MCP swipes use touch events
through Flutter's normal gesture arena because desktop scroll views exclude
mouse dragging by default. Named IME actions invoke the focused editor's
configured action and reject mismatches.

The supported target is the standard **single-window, single-view** Flutter
Windows runner. Platform views, CustomPainter contents and custom controls
without inspectable widgets/Semantics require native UIA or additional adapters.
Hit testing checks the element center; a partly covered control whose center is
covered is excluded. Elements outside the viewport appear after scrolling.

## Server discovery

The bridge binds an ephemeral IPv4 loopback TCP port. It writes a versioned
manifest to `%TEMP%/device-bot-flutter/<pid>.json`, containing that port, PID and
a random per-start token. No LAN listener is opened. The Rust Windows robot reads
only the launched target's PID (or foreground PID when no app was launched).
Both processes must use the same user/temp directory and interactive session.

The wire protocol is one newline-delimited JSON request/response per connection:

```text
request:  {"version":1,"token":"<manifest token>"}
response: {"version":1,"processId":1234,"elements":[...]}

request:  {"version":1,"token":"...","command":"imeAction","action":"next"}
request:  {"version":1,"token":"...","command":"swipe","startX":400,"startY":600,"endX":400,"endY":300,"durationMs":350}
response: {"version":1,"processId":1234,"ok":true}
```

Elements use the existing Windows adapter's flat schema: className, resourceId,
text, contentDescription, left/top/right/bottom, clickable, enabled, focused,
checked, selected, value. The server applies existing element-type/text filters
before maxResults. Snapshots are capped at 5,000 output nodes and 50,000 visited
widgets; the client caps responses at 4 MiB with a one-second discovery deadline.
Missing, stale or failed discovery bridges fall back to native UIA. A valid
empty snapshot stays empty. Crashed apps may leave manifests; unavailable
discovery endpoints are ignored. Widget actions have a three-second deadline;
failures are reported without replaying an action that may already have run.

## Validate

```text
flutter pub get
flutter analyze --no-pub
flutter test --no-pub
```

An opt-in Rust smoke test also exercises discovery, typed/key filtering, native
click/typing and UIA fallback against the running Windows sample. After building
and launching that sample in the interactive desktop, run from `server/`:

```powershell
$env:DEVICE_BOT_FLUTTER_SMOKE_PID = '<sample process ID>'
cargo test -p robot-windows flutter_live -- --ignored
```

Use a disposable sample session: this check inserts a Unicode string with spaces
and keyboard metacharacters into its email field.

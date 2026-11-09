# device_bot_flutter

Opt-in Flutter widget-tree discovery for Device Bot on Windows, macOS, Android,
iOS Simulator, physical iOS devices, and web. Native accessibility (DOM on web)
remains the primary source. The bridge is queried only when the selector has no
native matches, including controls whose native name/label/ID is empty.
Native errors, ambiguous matches, disabled controls, and truncated snapshots do
not silently switch to Flutter. Tap and text input still use platform drivers.
Existing Windows-only touch scrolling and IME actions remain unchanged.

```yaml
dependencies:
  device_bot_flutter:
    path: ../../packages/device_bot_flutter
```

```dart
import 'package:device_bot_flutter/device_bot_flutter.dart';

Future<void> main() async {
  await DeviceBotFlutter.start();
  runApp(const MyApp());
}
```

Start is idempotent; `await DeviceBotFlutter.stop()` unregisters the bridge.
Snapshots use release-compatible Flutter Element/render APIs. ValueKey<String>
and Semantics identifiers become resource IDs; widget types are included in
className so `class_contains: GestureDetector` can locate an unlabeled control.
No label is invented for an icon. Use a key, widget type, or row relationship;
multiple matches still require a more specific selector.
Password values are omitted, explicit empty values are preserved, and offstage,
transparent, clipped, and center-occluded controls are excluded.

## Desktop and web

Windows/macOS register an ephemeral loopback endpoint under
`<system temp>/device-bot-flutter/<pid>.json`, with a random per-start token.
The robot reads only the selected/foreground application's registration. macOS
uses a small native plugin to obtain the Flutter view's current desktop origin;
Windows uses Win32. macOS bounds are desktop points, Windows bounds are physical
pixels. The standard single-window/single-view runner is supported. Sandboxed
macOS apps need the network-server entitlement. If app and host temp directories
differ, configure an explicit port/token endpoint under `macos-host`, using the
same endpoint setup as mobile instead of relying on manifest discovery.

Web exposes a page-local `__deviceBotFlutterSnapshot()` function, consumed by the
Chrome driver through CDP with `awaitPromise`. No TCP listener or VM service is
needed. Bounds are CSS viewport pixels and remain independent of device DPR.
Custom embedding that scales, translates, or offsets the Flutter host is not
currently supported; use the standard full-page Flutter bootstrap.

## Android and iOS

Apps on devices cannot share the host's temp directory. Configure a token in
the app, and a device-specific endpoint on the host. Each endpoint must belong
to the app under test; use a different token for each app/session.

```dart
await DeviceBotFlutter.start(
  port: 39787,
  token: const String.fromEnvironment('DEVICE_BOT_FLUTTER_TOKEN'),
);
```

Pass a 32..128 character token through `--dart-define=DEVICE_BOT_FLUTTER_TOKEN=...`.
The default mobile port is 39787; override it when running multiple Simulator
apps, whose loopback ports share the host. A missing token generates a random
per-start token, as on desktop; configure one explicitly for host discovery.
Android release builds need the INTERNET permission. No LAN listener is opened.

For Android, forward a host loopback port to the app's port:

```sh
adb -s emulator-5554 forward tcp:39788 tcp:39787
export DEVICE_BOT_FLUTTER_ENDPOINTS='{"emulator-5554":{"version":1,"port":39788,"token":"<same app token>"}}'
```

For iOS Simulator, use its device ID as the JSON key and the app's listening
port directly. For a physical iPhone, first establish a USB loopback port tunnel
(e.g. using your existing device forwarding tool), then configure its host port
under that iPhone's exact Device Bot ID. The bridge does not create or own USB
tunnels. Never map a device to another application's endpoint.
Android bounds use physical pixels; iOS bounds use UIKit points.

## Query behavior

`mobile_list_elements_on_screen` accepts `source: auto|native|flutter` and returns
the source with its elements. `auto` uses Flutter only after native type/text
filtering returns no matches. The YAML runner applies its full selector,
including resource IDs and row relations, before deciding to query Flutter.
It negotiates source support, preserving compatibility with older MCP servers.
Trees are queried independently; native and Flutter nodes are never merged.

The protocol remains version 1: one authenticated newline-delimited JSON
request/response per connection. Discovery has a one-second host deadline,
4 MiB response limit, 5,000 output nodes and 50,000 visited widgets. Invalid or
stale desktop registrations are unavailable, rather than replacing native data.
Platform views and custom drawing without inspectable widgets are not exposed.

## Migration and validation

`packages/uia_windows` is a compatibility package exporting the deprecated
`UiaWindows` alias. New code should use `DeviceBotFlutter` and the new package.

```sh
flutter pub get
flutter analyze --no-pub
flutter test --no-pub
```

# Device Bot Companion

The optional Flutter desktop companion connects to the local Device Bot server's
MCP endpoint at `/mcp`, using the same initialize/session/tools contract as any
other MCP client. It displays devices, local MCP activity, and on-demand device
screenshots. The screenshot inspector also finds all accessibility
elements, overlays their bounds and names, and provides a small `robot-flow` YAML
editor for drafting a scenario around the selected element. It never sends
device-control requests through a separate admin protocol.

For iOS Simulator, the target app must first be launched through Device Bot
(for example with `mobile_launch_app`) before the inspector can query XCTest
accessibility elements. The inspector keeps the screenshot visible and shows
this recovery hint when no active app is known.

Normally, starting the Rust server in this repository automatically launches
this Flutter project after the HTTP listener is ready. To run the companion
manually instead:

```bash
cd ..
DEVICE_BOT_FLUTTER_AUTOSTART=0 \
  cargo run -p device-bot -- --listen 127.0.0.1:3337
```

The server prints the generated bearer token. Then run this Flutter project
with the same URL and token, for example:

```bash
flutter run -d macos \
  --dart-define=DEVICE_BOT_URL=http://127.0.0.1:3337 \
  --dart-define=DEVICE_BOT_AUTH=replace-with-the-printed-token
```

The companion is intentionally an MCP client only. It does not start or stop
the Rust server and never controls devices outside the `/mcp` session.

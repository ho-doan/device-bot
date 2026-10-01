# Device Bot Companion

The optional Flutter desktop companion reads the local Device Bot server's `/admin/*`
API. It displays devices, public server configuration, live MCP activity, and
on-demand device screenshots. It never sends device-control requests itself.

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

The companion can alternatively launch the local `device-bot` binary:
enter its executable path, the desired local URL, and token, then select
**Start server**. It only stops a process that it launched itself; attaching to
an existing server remains read-only and safe.

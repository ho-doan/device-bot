# Device Bot Companion

The optional Flutter desktop companion reads the local Device Bot server's `/admin/*`
API. It displays devices, public server configuration, live MCP activity, and
on-demand device screenshots. It never sends device-control requests itself.

Start the Rust server first:

```bash
cd ..
DEVICE_BOT_AUTH=local-secret cargo run -p device-bot -- --listen 127.0.0.1:3337
```

Then run this Flutter project and enter the same base URL and bearer token.
The companion can alternatively launch the local `device-bot` binary:
enter its executable path, the desired local URL, and token, then select
**Start server**. It only stops a process that it launched itself; attaching to
an existing server remains read-only and safe.

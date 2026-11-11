# Orbit control smoke

Shared tap/input flow for the Orbit clones. The login actions come from the
Flutter fixture's IT-02/IT-03 flows. This release checks device control; full
validation, routing, checkbox-state and IME assertions remain in
[`../flutter_app/automation_test`](../flutter_app/automation_test).

Build/install the fixture first and cold-stop it before every native run. On
Android use `adb -s DEVICE shell am force-stop APP_ID`; on iOS Simulator use
`xcrun simctl terminate DEVICE APP_ID`. The profile does not clear app storage.
The smoke requires empty login fields and literal disposable demo credentials.

```sh
server/target/debug/robot-flow \
  --profiles examples/automation_test/targets.yaml --target swift \
  --device SIMULATOR_UDID \
  --server server/target/debug/device-bot --server-arg mcp \
  --confirm fixture_ready --artifacts artifacts/control-smoke/swift \
  examples/automation_test/control-smoke.yaml
```

Targets: `swift`, `android`, `rn-android`, `rn-ios`, `vite`, `electron`.
Override bundle IDs, executable paths or URLs with `--app-id` as needed.
For web, start Vite and Chrome with a separate test profile and
`--remote-debugging-port=9222`, then pass the exact `chrome:TARGET_ID` from
`http://127.0.0.1:9222/json/list`. The default Vite URL is port 5173; override
it if Vite selects another port. A fresh full navigation resets the demo session.

For Electron, start the example with:

```sh
cd examples/elec-app
npm ci --no-audit --no-fund
npm run dev -- -- --remote-debugging-port=9223
```

Set `DEVICE_BOT_CHROME_DEBUG_ADDRESS=127.0.0.1:9223` on the runner and use
that endpoint's exact Chrome target ID. `--app-id` is the renderer's login URL.
This exercises the actual Electron window through CDP.

Run MCP sessions sequentially. Use `--dry-run` to validate a profile without
touching devices. Successful smoke execution means the driver accepted all
tap/input calls and captured screenshots; it does not assert application
business behavior. Reports, queries and PNGs are written under `--artifacts`.

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
  --profiles examples/automation_test/targets-swiftui.yaml --target swift \
  --device SIMULATOR_UDID \
  --server server/target/debug/device-bot --server-arg mcp \
  --confirm fixture_ready --artifacts artifacts/control-smoke/swift \
  examples/automation_test/control-smoke.yaml
```

SwiftUI uses [`targets-swiftui.yaml`](targets-swiftui.yaml), target `swift`.
Android Compose uses [`targets-compose.yaml`](targets-compose.yaml), target `android`.
The other targets in `targets.yaml` are `rn-android`, `rn-ios`,
`vite`, and `electron`.
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

## SwiftUI setup and field differences

Keep the Simulator's XCTest helper ready on the same bridge address before a
control run. A cold helper can take longer than the tap search's 30-second
deadline. An unfinished first query then appears as a missing/disabled/unstable
element even when the field ID is correct. A failed `/health` heartbeat is a
helper-readiness problem; changing a field label does not repair that problem.

The SwiftUI profile sends both `filter` and `element_type` to XCTest. A
`resource_id` alone is checked locally by robot-flow and does not narrow the
native query. Email uses `email-field`; both SwiftUI `SecureField` and visible
`TextField` use `password-field`, normalized as `textField`. The email editor's
native label is not the separate `Email address` static text. Buttons use their
own identifiers, and `native_tap` targets XCTest elements rather than cached
screen coordinates. The initial `list_elements` query runs outside the tap
deadline and exposes helper setup/query errors directly.

Validated on iOS 27 Simulator: the dedicated profile completed email/password
input, visibility toggles and Sign in. Captured screenshots showed the entered
demo credentials and the resulting workspace. Full business assertions remain
deferred. This confirms a ready-helper run, not a cold-helper startup guarantee.

## Android Compose fields

The example root enables `testTagsAsResourceId`, exposing the existing Compose
`testTag` values to UI Automator. Without it, the native editors have no resource
ID and their visible labels are separate nodes. Compose buttons similarly expose
the clickable `android.view.View` separately from label and role children. The
Compose profile selects the resource ID on that clickable node, without requiring
its class to normalize as `button` or its label to be present on the same node.

Validated on a physical Samsung SM-G960F, Android 10, over USB using the checksum
verified 0.1.9 native release: credential input, visibility toggles and Sign in
completed, and the final screenshot showed the workspace.

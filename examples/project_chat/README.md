# Project Chat fixture

A local, disposable target app for two-account automation. It uses in-memory data; restarting resets conversations and sessions. It is not a production chat service: credentials are demo values, HTTP is unencrypted, and the server should only be used on a trusted local host.

```sh
cd examples/project_chat
npm ci
node server.js
node --test server.test.js
```

The default address is `http://127.0.0.1:4173`; set `HOST` and `PORT` to change the listener. The app has accounts `alice` / `alice-demo` and `bob` / `bob-demo`. Copy `.env.example` to `.env` for the project, or export the values in the process environment. `CHAT_URL` must be reachable from the Chrome host. Keep real credentials out of flow files and evidence: recording and screenshots may reveal input values.

`project.yaml` describes macOS; change both the host platform and desktop resource kind for Windows. `chat.yaml` is one case with explicit `alice` and `bob` resource scopes, and checks both directions in the same conversation. The single flow binding deliberately schedules one case; resource scopes address both prepared actors. Resources must have separate browser contexts or profiles. Two ordinary tabs sharing cookies are insufficient. No target IDs are embedded in the manifest.

The current resource declarations do not themselves guarantee isolated Chrome profiles; the project runtime must support and verify this before the example can serve as live evidence. A successful dry-run only validates syntax. Start the target app separately; project YAML does not execute shell commands. Run `device-bot run --dry-run project.yaml` and, when the required actor capability exists, `device-bot run project.yaml`. The fixture tests validate actual HTTP authentication, SSE and WebSocket payloads, but do not establish Device Bot observer coverage or a live UI pass.

## Permission exercise

`permission.yaml` is a separate, pending native-interaction scenario. It expects Alice already signed in, a fresh profile with notification permission at `default`, and a desktop resource representing the host displaying Alice's Chrome window. Add it as a bound project flow only when those prerequisites can be met. The page button invokes the real `Notification.requestPermission()` API; it does not simulate or pre-grant permission. The `Allow` step targets native desktop accessibility, not page HTML. Browser permissions differ from HTML modals, JavaScript dialogs and OS dialogs; actual labels and accessibility exposure vary by browser and host.

Chrome incognito contexts can automatically deny notifications without displaying an actionable prompt. Therefore an isolated incognito actor is not sufficient evidence for a native Allow interaction. Use a profile-isolated actor capable of showing the prompt and an interactive host/native route; verify the actual prompt and returned `granted` state. No permission interaction, desktop recording, or observer milestone is claimed by this fixture alone.

SSE is at `/api/events` and WebSocket at `/api/socket`; both require a session cookie and receive a JSON `message` event containing the conversation, sender, ID and text. The browser consumes both and deduplicates by message ID. Logging out invalidates that session and closes its event connections. Conversations are created on first contact selection, reused for the same pair, and accessible only to their participants. The server's conversations, message history and sessions exist only for the lifetime of its process.

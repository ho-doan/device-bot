# Live setup

## Web recovery and Live

QC and YAML use the same Chrome `boot_device` path. **Start** automatically
prepares Remote Debugging: it reuses an available Chrome debugger, or launches
Chrome with a Device Bot profile and a free loopback debugging port. No manual
`chrome://inspect` setup is needed for this automatic path. Chrome's normal
profile and its open windows are kept intact. The separate profile is required
by Chrome 136+ when debugging is enabled with command-line flags.
Set `DEVICE_BOT_CHROME_PATH` to override the executable. Set
`DEVICE_BOT_CHROME_DEBUG_ADDRESS` or `DEVICE_BOT_CHROME_ACTIVE_PORT_FILE` to
explicitly attach to an existing debugger; these overrides disable automatic
browser startup. An unrelated service on a port is never stopped.

**Restart web session** sends `mobile_boot_device` with `restart: true`. It disposes
only this MCP client's private case context, then creates a new one in the same
browser. Other case contexts and personal browser windows remain open. Closed or
stale case contexts are also recovered by the normal Start/YAML boot path.

For Chrome devices, the QC launch button is **Open URL**. Enter the target HTTP(S)
URL in QC to navigate the current private case through `mobile_open_url`; no manual
address entry in Chrome is required. Other platforms retain the Launch app picker.

Web **Live control** prefers browser-native `getDisplayMedia` and WebRTC on
supported HTTPS/localhost pages. Before requesting capture, the bot activates
the controlled tab and enables CDP focus emulation for the live session; cleanup
disables focus emulation again. In Chrome, choose **This Tab** and allow sharing.
Region Capture binds the stream to the controlled document, rejecting a
mismatched tab selection. In this native path the browser sends video directly
to Flutter without Rust re-encoding it.

When the page lacks the capture APIs, or capture cannot start because Chrome
reports missing focus/transient activation (`InvalidStateError`), Live uses CDP
screencast events and FFmpeg/libx264 through the shared H.264 WebRTC gateway.
It does not poll screenshots. A sharing permission denial (`NotAllowedError`)
remains an error and does not switch capture methods. Reopen Live after navigation
or resizing. Sharing ends on Stop, session shutdown, navigation, or peer loss.

**Current limitation (2026-11-12, macOS / Chrome 157):** Google HTTPS Live still
fails to display video in QC in the observed run, even though automatic tab focus
works. This remains deferred; passing logic tests does not establish end-to-end
video support. See the [follow-up note](../../.planning/notes/2026-11-12-chrome-web-live-video-limit.md)
for observations and the remaining verification.

DOM inspection and inputs use the same MCP session as the private case (the root
admin session cannot access another case). Select inspects and suggests YAML;
Tap selected controls the page; drag scrolls; Send text and Clear web field act on
the focused input. Only DOM metadata is refreshed periodically.

## Android realtime viewer

Android Live uses scrcpy-server (MediaCodec H.264), ADB, the shared Rust
`robot-stream` WebRTC gateway, and `flutter_webrtc` in the QC companion.
There is no screenshot polling. MCP handles SDP negotiation and lifecycle:
`mobile_start_stream` accepts `deviceId` and a complete, ICE-gathered receive-only
video SDP `sdp`; its structured result contains `streamId`, answer `sdp`, `width`,
and `height`. Apply the answer to the viewer peer. `mobile_stop_stream` accepts
`streamId` and is idempotent. Streams belong to the MCP session and stop on
session close, viewer disconnect, capture failure, or a 20-second connection timeout.

The release installers include matching scrcpy client/server **5.0.1** (tested
on Samsung Android 10). For source builds, install **5.0.1** separately, or
use the **3.3.4** protocol adapter. Homebrew and standard Linux server locations
are detected automatically. For portable/custom installations set both
`SCRCPY_SERVER_PATH` (host jar path) and `SCRCPY_SERVER_VERSION` (exact version).
Unsupported versions fail explicitly because the scrcpy wire protocol is versioned.
The server pushes the jar and creates a per-stream ADB forward; closing capture
releases the process, socket and forward. Keep the launch arguments minimal:
redundant defaults caused a native Samsung encoder crash during device testing.

The viewer connects directly to Rust using local/LAN ICE candidates (no external
STUN/TURN configured). Video is not decoded/re-encoded by Rust. Inspection metadata
travels separately over authenticated SSE; taps/swipes/text use the authenticated
admin input endpoint. Clicking the video or a list item selects an element for
inspection only, including disabled/non-clickable nodes. `Tap selected` explicitly
sends a device tap and is enabled only for an enabled, clickable selection.
The inspector previews robot-flow YAML (tap, assert, text or scroll) with a Copy
YAML action; copying never controls the device. Dragging the video still scrolls.
`Send text` types into the focused Android editor. `Clear Android field` clears
that editor through the existing scrcpy control socket using Ctrl+A then Delete;
it is disabled without an enabled, focused editor, and the server rechecks focus.
Clearing Android does not delete the QC text draft.

To run the physical-device regression test:

```sh
DEVICE_BOT_TEST_ANDROID=<adb-serial> cargo test -p robot-stream physical_device_stream_arrives_over_webrtc_and_stops -- --ignored --nocapture
```

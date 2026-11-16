# Windows accessibility: UIA and MSAA

`robot-windows` includes an opt-in direct MSAA/`IAccessible` reader. It runs in
the existing isolated PowerShell helper; no new crate or app package is needed.
UIA remains the default. Set `DEVICE_BOT_WINDOWS_ACCESSIBILITY=msaa` **before
starting Device Bot/QC** to use MSAA for native element queries, including Live.
Use `uia` or unset the variable to restore UIA. Invalid values are errors.

MSAA obtains `OBJID_CLIENT` through `AccessibleObjectFromWindow` on the target
HWND and its same-process child HWNDs (Flutter may expose its provider on the
child view). It enumerates both object children and integer child IDs, mapping
roles, names, descriptions, values, physical desktop bounds and observable states
to the existing element schema. Queries do not activate the app or invoke actions.
Window-scoped targets validate PID ownership and never search another window.
PID-only queries select the first visible, restored top-level window of that PID;
use a `windows:<pid>:<hwnd>` target when an app has multiple windows.

The two trees are queried independently, never merged. `source: native` selects
the configured native backend; `source: flutter` still explicitly requires the
Flutter bridge. `source: auto` retains the existing native/Flutter fallback, so
**use native-only queries when evaluating whether the bridge can be removed**.

## Compare a real Flutter release app

Build/run a release app without `DeviceBotFlutter.start()`. Obtain its PID and
HWND from Device Bot's running-app/window listing. Run from `server/` in Windows
PowerShell, replacing the example target with the app's actual identifiers:

```powershell
$targetId = 'windows:1234:5678'
$env:DEVICE_BOT_WINDOWS_ACCESSIBILITY = 'uia'
cargo run -p robot-windows --example accessibility_snapshot -- $targetId > uia.json
$env:DEVICE_BOT_WINDOWS_ACCESSIBILITY = 'msaa'
cargo run -p robot-windows --example accessibility_snapshot -- $targetId > msaa.json
```

The example calls native discovery directly, never the Flutter bridge. Repeat
on each screen/menu/dialog and after scrolling. If the tree is sparse, retry
with Windows Narrator/NVDA enabled to check whether accessibility activation is
the missing condition. Record that requirement if it is necessary on your app.

For a repeatable assertion on the current screen:

```powershell
$env:DEVICE_BOT_MSAA_PID = '1234'
$env:DEVICE_BOT_MSAA_HWND = '5678'
$env:DEVICE_BOT_MSAA_LABELS = '["Email","Sign in"]'
cargo test -p robot-windows msaa_release_controls_without_bridge -- --ignored
```

This assertion uses MSAA directly, irrespective of the backend environment
variable. Choose labels actually visible in your app. A label assertion proves
discovery only; it does not prove typing, clicking or complete scenario coverage.

The bridge can be removed for a workflow only after all required controls have
usable selectors/bounds and native input completes the workflow. Check editable
fields, icon-only buttons, custom gestures, popup menus, dialogs, scrolling and
checkbox/radio state. Use type/text/relationships available from accessibility;
do not assume MSAA exposes Flutter `ValueKey` or widget class names.

## Limits and validation

- MSAA exposes the app's semantics, not the complete widget tree. Merged,
  unlabeled and custom-painted widgets may remain inaccessible. No stable
  resource ID is invented from the snapshot-local MSAA child ID.
- Invisible/offscreen nodes are excluded. Password/protected text is not queried;
  if state cannot be read, values/names are withheld and enabled is conservative.
  Mixed checkbox state and unavailable state remain unknown.
- Traversal is capped at 5,000 visits, depth 128 and the requested result limit.
  A 10-second external deadline kills a hung helper. These limits can produce a
  partial tree; absence in a large snapshot alone does not prove no semantics.
- MSAA does not replace bridge-specific IME actions or touch swipes. Native
  pointer, keyboard and drag input retain their existing behavior.
- An interactive Windows session and compatible elevation are required. This
  reader does not bypass secure desktops or cross-session access restrictions.

`cargo test -p robot-windows --all-targets` runs Rust tests and, on Windows, managed
IAccessible provider contracts for child IDs/object children, cycle and visit
limits, roles, filters, state and protected values. The ignored release test
requires the app and environment variables above. Actual `oleacc.dll` behavior
and Flutter release coverage require Windows validation; tests on macOS do not
establish that the bridge can be removed.

References: [AccessibleObjectFromWindow](https://learn.microsoft.com/en-us/windows/win32/api/oleacc/nf-oleacc-accessibleobjectfromwindow),
[AccessibleChildren](https://learn.microsoft.com/en-us/windows/win32/api/oleacc/nf-oleacc-accessiblechildren),
[MSAA state flags](https://learn.microsoft.com/en-us/windows/win32/winauto/object-state-constants).

## Desktop recording

Screen recording captures the visible Windows desktop independently of the
selected app or Live window. FFmpeg must support `gdigrab` and `libx264`; the
Windows bundle is discovered next to Device Bot, or set
`DEVICE_BOT_FFMPEG_PATH` to an executable. An interactive desktop is required.
Only visible content is recorded; hidden tabs or remote devices need a visible
window or mirror. Start returns the intended output path after the first frame;
stop finalizes the MP4 and returns the saved path. Session cleanup also stops
and finalizes its own active recorder. A failed capture or finalization reports
an error, and recordings belonging to other sessions are not stopped.

Automated process tests do not establish actual Windows capture or playback;
verify the resulting video on the Windows host.

# Platform parity

Device Bot registers six drivers at startup. Shared MCP tools use the same
argument and result contracts, while drivers own their native prerequisites,
target selection, and lifecycle. See the [architecture](architecture.md)
and [driver assembly](../crates/mcp-server/src/bootstrap.rs).

## Platform matrix

The matrix describes implemented paths. On a selected target, use
`mobile_get_capabilities` to check the readiness the driver can establish.
Readiness can differ between the host, a window, a device, and its selected
app. See [capability preflight](architecture.md#device-capability-preflight).

| Driver | Element source | Explicit document scope | System keyboard exclusion | Screen recording |
| --- | --- | --- | --- | --- |
| Android | UI Automator XML via ADB | Unsupported | Unsupported | ADB `screenrecord`, with `setsid` for ownership |
| iOS Simulator | XCTest UI runner | Inspectable WebView | Supported by the native XCTest query | `simctl io recordVideo` |
| Physical iOS | Signed XCTest agent | Inspectable WebView with `appId` | Unsupported | QuickTime with a supported device connection |
| macOS | Native AXUIElement for the selected app or window | Unsupported | Unsupported | ffmpeg with AVFoundation and screen permission |
| Windows | UIA raw view or the opt-in MSAA provider | Unsupported | Unsupported | ffmpeg with gdigrab on an interactive desktop |
| Chrome | DOM via CDP | Page and frame selectors | Unsupported | CDP screenshots encoded by ffmpeg |

Android and iOS Simulator support installing and uninstalling apps. Physical
iOS uses `go-ios` for install and `xcrun devicectl` for uninstall. Desktop
and Chrome app install/uninstall return `Unsupported`. macOS and Windows
implement native drag through the swipe tool. Browser runtime and network
observation are Chrome-only and scoped to the attached page.

## Element query contract

Match case-insensitive text and normalized element type before applying
`maxResults`, preserving source order. The text fields include class,
resource ID, label, description, and value when the source provides them.
An explicit unsupported scope or keyboard exclusion request returns
`Unsupported`.

- Android normalizes the UI Automator snapshot, and Chrome normalizes DOM
  nodes for the chosen document or frames. Both apply the portable query.
- The iOS drivers pass native limits and filters to XCTest and apply the
  portable query to the decoded results. Scoped WebView queries use the
  Web Inspector.
- macOS filters during AX traversal for both host and window targets. The
  traversal retains its depth and time budgets.
- Windows applies the portable query to UIA and MSAA snapshots in both host
  and window paths. The helpers cap the snapshot at 5,000 nodes and depth
  128 with a 10-second helper deadline. MSAA also caps visited nodes at
  5,000. Filtered queries search within these budgets. Unfiltered queries
  can stop at the caller's requested count. The backends use separate native
  providers, with UIA selected by default. [Windows validation guide](../crates/robot-windows/README.md)

## Required parity pass

1. Check each shared operation against its portable contract and its selected
   target's readiness. Document unsupported options and relevant prerequisites.
2. Use parser fixtures for native payloads and assert portable fields, filter
   order, result limits, and typed errors in driver and MCP tests.
3. Run the same MCP scenarios on the relevant native targets, including an
   Android emulator and an iOS Simulator for mobile parity. Use an interactive
   Windows or macOS session for desktop UI and a Chrome page for DOM/frame
   behavior. Record screenshots or video when the scenario needs evidence.

The [automation workflow](../../.github/workflows/automation-tests.yml) runs
web scenarios on Linux and Windows and provides a manually triggered iOS
Simulator job. Use the [automation guide](../automation_test/README.md) for
the runtime prerequisites and replay procedure.

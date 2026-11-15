# QC JSON contracts and remaining adapters

This refactor makes schemas discoverable and keeps wire decoding out of Cubits,
widgets and rendering code. It does not equate every `Map` or validation branch
with an incomplete migration.

## Generated contracts

| Contract | Model source | Consumers |
| --- | --- | --- |
| Native bridge request, registration, response and native/web widget snapshot | `packages/device_bot_flutter/lib/src/bridge_protocol.dart` | Native socket bridge, browser discovery bridge, widget tree capture |
| MCP JSON-RPC envelopes, errors, content, tool results and device/app/window/display lists | `server/flutter_app/lib/core/models/mcp_models.dart` | MCP HTTP client |
| Device/display/bounds/elements and dashboard data | `server/flutter_app/lib/core/models/admin_models.dart` | Dashboard, inspector and device dialogs |
| Runner status and diagnostics | `server/flutter_app/lib/core/models/runner_models.dart` | Readiness validation and recovery UI |
| Installed app metadata | `server/flutter_app/lib/core/models/installed_app.dart` | Installed app selector and platform metadata adapter |
| Page/frame context | `server/flutter_app/lib/core/models/web_context.dart` | DOM selectors, frame scoping, recorded YAML |
| Live input, stream answer/context, frames and element snapshots | `server/flutter_app/lib/core/models/live_models.dart` | MCP input, WebRTC, LiveCubit and recording |
| Flow report summary | `server/flutter_app/lib/core/models/flow_report.dart` | Flow execution and recording review |
| Local connection profile | `server/flutter_app/lib/core/models/local_connection_profile.dart` | Local bearer discovery |

Generated `.g.dart` files are committed and checked for drift in CI. Each package
runs its own `build_runner`; the QC build does not generate source inside a path
dependency. `admin_models.dart` re-exports the split models to keep existing
callers working.

## Enums and unsupported values

| Enum | Wire values / policy |
| --- | --- |
| `BridgeCommand` | `imeAction`, `swipe`; unknown commands are rejected before invoking the action callback |
| `BridgeImeAction` | `next`, `done`, `go`, `search`, `send`; unsupported actions fail explicitly |
| `McpContentType` | Known MCP content types; unknown blocks do not prevent reading supported text/image blocks |
| `LiveEventType` | SSE `frame`, `elements`, `error`; other event names are ignored |
| `LiveInputAction` | `tap`, `scroll`, `swipe`, `type`, `clear`; Dart `typeText` serializes as `type` |
| `RunnerReadinessState` | Known helper recovery states use snake_case on the wire; an unknown state cannot grant readiness |
| `FlowRunStatus`, `FlowStepStatus` | Known run/step results; unknown statuses display a fallback and do not become PASS |
| `InspectorTreeSource` | The finite `auto`/`native`/`flutter` choice is typed through inspector widgets, Cubit and callbacks; the MCP boundary uses its wire name |
| `RunnerAction`, `InspectorFlowAction` | Existing application action enums remain separate from wire status enums |

## Remaining manual code and why it remains

These are intentional adapters or compatibility boundaries, not limitations that
prevent using `json_serializable` for the fixed schemas above.

### 1. Open MCP payloads and tool argument construction

`McpRpcRequest.params`, `McpRpcResponse.result`, `McpToolCall.arguments` and
`McpToolResult.structuredContent` remain JSON maps in `mcp_models.dart`. MCP uses
a different schema for each method/tool, including extensions. A single global
DTO for every result would couple the client to every current and future tool.
The client dispatches these maps into specific generated DTOs at the point where
the tool schema is known. It also constructs tool argument maps in
`mcp_client.dart` and `live_recording.dart`; those maps represent the open MCP
argument contract used by robot-flow YAML.

The MCP/session exception retains the raw device recovery object so callers can
inspect diagnostics that are not yet modeled. This is not handwritten field
parsing in Cubits or widgets.

### 2. Platform app-list containers and metadata aliases

`core/models/installed_apps_decoder.dart` still examines lists, nested envelopes
(`apps`, `applications`, `installedApps`, `result`, `data`) and dictionaries keyed
by bundle identifier. Platform discovery tools do not return one uniform root
schema. Choosing the correct container therefore remains an explicit adapter.
Every selected entry then uses the generated `InstalledApp.fromJson` factory.

`installed_app.dart` uses `JsonKey.readValue` helpers for aliases such as
`bundleId`, `CFBundleIdentifier` and `packageName`. Alias precedence, trimming and
fallback names are compatibility policy; a plain generated field mapping cannot
infer that policy. The helpers are centralized beside the schema rather than
repeated in UI/network consumers.

### 3. Tolerant Live nodes and optional page/frame data

`live_models.dart` keeps `_elementsFromJson` / `_elementOrNull` as a custom field
converter. It skips malformed Live nodes while retaining each usable node's
original snapshot index. The default generated list conversion would fail the
whole update on one malformed node. Compacting indices after skipping nodes
would point positional selectors at the wrong controls. Normal MCP inspection
uses the separate strict `ElementSnapshotPayload` decoder.

`web_context.dart` keeps `_framesFromJson` and `_pageOrNull`. They preserve the
existing behavior of ignoring non-object frame entries and non-object optional
page data. Valid objects use generated factories. These are explicit compatibility
converters, not a second handwritten implementation of the schema.

### 4. Platform context, diagnostics and existing map-facing APIs

`LiveStreamAnswer.context`, `AndroidVideoSession.recordingContext`,
`LiveRecording.context`, runner diagnostics and a few public callback/method
boundaries still retain maps. Native windows, mobile capture and browser capture
can attach different metadata; unknown keys must survive forwarding, recording
and recovery. Known fields are consumed through `LiveStreamContext` and
`RunnerDiagnostics`, with generated parsing. Replacing the retained map with
only known DTO fields would silently discard platform extensions.

The bridge's existing `WidgetTreeSnapshot.capture` helper also keeps its map
return type for callers. It delegates to typed node capture and generated
`toJson`; it does not maintain a second handwritten serializer.

The existing `startAndroidStream`, `runFlow`, input callback and report formatter
signatures remain compatible with their callers. Their fixed fields are decoded
through DTOs before use. Migrating those signatures to DTO-only APIs is possible;
it is deferred rather than claimed as technically impossible. It would require a
separate caller/API migration and a defined policy for retaining extension data.

### 5. Validation and non-JSON framing

`json_value.dart` and the bridge protocol decoder still check that decoded JSON
has an object root. Bridge authorization still checks version/token on a typed
`BridgeRequest`; generated deserialization cannot decide whether a credential
matches the running instance. Local connection discovery still validates
loopback host, scheme and port before using a token. Runner readiness still
checks helper versions and target app identity after decoding.

The native bridge still implements bounded newline-delimited framing, and the
MCP client still parses SSE `event:` / `data:` lines. These are transport protocols
around the JSON payload. `json_serializable` converts their JSON objects; it does
not replace socket limits, cancellation, SSE framing or application validation.

### 6. Framework/plugin maps and YAML generation

Dio headers/options and flutter_webrtc configuration use the map-based APIs of
those libraries. Wrapping each in another repository DTO would add translation
without changing the wire-decoding contract. YAML templates, `jsonEncode` of YAML
scalar values and generated MCP argument blocks remain in the recorder/inspector;
JSON model generation is not a YAML serializer.

Identifiers, labels, class names, tool names and diagnostic detail strings remain
strings. They are external/open values, not finite action/status sets.

### 7. Existing string-based UI/domain interfaces

`DeviceReadiness.state` and `AdminDevice.platform` still expose strings for compatibility with their existing callers. Wire
runner states are decoded through `RunnerReadinessState`, and unknown states
cannot report readiness. The new command/status enums do not imply every existing
UI interface has been converted to an enum.

Driver platform names are external identifiers; preserving their raw strings lets
an unfamiliar driver still be listed. The domain readiness object keeps its
existing string-facing API, while the wire model and recovery paths use the
status enum. These interfaces are intentionally retained for compatibility,
not reported as a completed enum migration. A later DTO-only caller migration
can change them; there is no tool/dependency blocker.

## Verification scope

CI checks bridge and QC code generation, analysis and tests, then the Chrome
stream lifecycle and QC web build. The existing web YAML suite runs on Linux and
Windows. Focused tests cover rejected bridge credentials/versions/commands,
registration ownership, unknown enums, malformed Live nodes and their indices,
nested web context and preservation of platform extension metadata.

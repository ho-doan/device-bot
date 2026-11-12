# YAML scenarios through MCP

`device-bot run` delegates to the bundled `robot-flow` Rust MCP stdio client.
It starts the configured `device-bot mcp`
binary, completes `initialize` / `notifications/initialized`, reads `tools/list`,
then executes the scenario through `tools/call`. It has no platform-driver or
`RobotRegistry` dependency. The MCP server and tool schemas are unchanged.

## Run

The native release bundle includes the runner. The test host does not need Cargo:

```sh
device-bot run scenario.yaml
device-bot run --dry-run scenario.yaml
```

The default server is `device-bot mcp` (or `DEVICE_BOT_BIN` with argument `mcp`).
An explicit `--server` override uses only the supplied `--server-arg` values;
when overriding with a Device Bot path, add `--server-arg mcp`.

To build the client from source:

```sh
cargo build --manifest-path server/Cargo.toml --release -p robot-flow
server/target/release/robot-flow --help
```

Validate the nine Orbit scenarios without starting a server or touching a device:

```sh
server/target/release/robot-flow --dry-run \
  --profiles examples/flutter_app/automation_test/targets.yaml \
  --target ios-sim examples/flutter_app/automation_test/it-0*.yaml
```

For a live run, install the matching fixture app, cold-stop it, and keep the
device/session available. Fixture startup/reset is external setup because the
current MCP contract has no general terminate/reset operation. On Windows, start
the fixture and its bridge using the existing fixture setup before running.
`--confirm fixture_ready` records operator confirmation of that setup; it does
not perform it or prove a cold start. Prepare the fixture again before each case.

```sh
DEVICE_BOT_FLUTTER_AUTOSTART=0 server/target/release/robot-flow \
  --profiles examples/flutter_app/automation_test/targets.yaml --target ios-sim \
  --device "$DEVICE_ID" --confirm fixture_ready \
  --server /path/to/device-bot --server-arg mcp --artifacts artifacts/yaml \
  examples/flutter_app/automation_test/it-01.yaml
```

Targets are `android`, `ios-sim`, `ios` (physical iPhone), and `windows`. Set
`ORBIT_APP_PATH` or pass `--app-id` for the Windows fixture executable. Use
`--device` when more than one device matches. Selection always requires exactly
one match. Physical iOS still needs its signed XCTest runner and reachable MCP
bridge; Simulator validation is not physical-iOS validation.

Request, step and case deadlines are bounded. Configure `--rpc-timeout` and
`--case-timeout` in seconds. Individual steps accept `timeout_ms` where documented
below; query/tap polling has a bounded attempt count. Ctrl-C/SIGTERM attempts
recording cleanup and closes the server process. An unresponsive transport can
prevent graceful recording cleanup; that failure is reported, not hidden.

When an outer step deadline cancels a stdio request, the transport preserves
partially read JSON lines and drains late responses only for known outstanding
request IDs. This keeps recording cleanup correlated with its own response.
Unrelated IDs and actual transport/RPC timeouts still fail explicitly.

## One scenario, target profiles

The v2 scenario contains the sequence and assertions. Target/profile YAML contains
app/device configuration, logical selectors and declarative recipes. Root
`selectors`/`recipes` are shared defaults. A target overrides a whole named
selector or recipe, not individual fields within a selector.

```yaml
version: 2
name: login
steps:
  - id: device
    select_device: ${device}
  - id: launch
    launch_app: {app_id: '${app_id}'}
  - id: email
    fill_field: {target: email, text_from_env: TEST_EMAIL}
  - id: submit
    tap_element: {target: sign_in}
  - id: heading
    assert_element: {label: A fresh start, Alex., timeout_ms: 15000}
```

Every step has exactly one action. IDs are optional but should be stable for
source traceability. Unknown actions/arguments, duplicate IDs, missing references,
recipe/relation/variable cycles and unresolved variables fail validation.
`${app_id}`, `${device}`, profile `variables` and `${env:NAME}` are resolved as
data, never shell commands. `run: {recipe: NAME, with: {...}}` passes parameters
to a recipe. Recipe parameters preserve literal whitespace, case and Unicode.
Recipe and conditional branch parameters are resolved once; literal `${...}`
inside environment-sourced data is preserved through nested branches.
Recipes and `if_element` branches use the same primitives as scenarios.

## Platform for one step

An optional `platform` beside the action routes that step to `android`, `ios`,
`macos`, `windows`, or `chrome`. Omitting it keeps the selected device (or the
parent step's platform inside a branch/recipe). The override does not change
`select_device` or the device used by subsequent steps.

```yaml
version: 2
name: chrome_notification
steps:
  - select_device: {id: 'chrome:<target-id>'}
  - tap_element: {label: Enable notifications}
  - platform: macos
    tap_element: {label: Allow}
  - assert_element: {label: Notifications enabled}
```

For Chrome on Windows, use `platform: windows` on the popup step. Replace the
example target ID and labels with those from your environment. Chrome clicks
activate the selected tab before dispatching input, so the next desktop step can
read its native permission popup even if another app was in front. Native browser
popups must be exposed by macOS Accessibility or Windows UIA; this routing feature
does not add them to the page DOM. Native input requires the host's existing
permissions and an interactive desktop.

Routing uses the same MCP session and advertised `mobile_*` tools, passing the
chosen `deviceId` so the server dispatches to the appropriate driver. A matching
explicitly selected device or parent scope is reused; otherwise the runner calls
`mobile_list_available_devices` and requires exactly one device for the platform.
Zero/multiple matches fail before the action. With multiple Chrome tabs, first
use `select_device` with the exact `chrome:<target-id>`.

`platform` also supports profile/recipe variables. A platform on `run` supplies
the default for its expanded recipe steps; a platform on `if_element` covers the
condition and its branch. Child steps may override it. Use `select_device` outside
these scopes to change the persistent selection. `call_tool` still names the
exact MCP tool; with a platform scope, omit `arguments.deviceId` or use
`"<selected>"` to let routing supply it. A literal device ID is rejected during
preflight to prevent conflicting routes. The tool must accept `deviceId`.

Recording cleanup always uses the device that started recording, even after a
platform override or an error. An explicitly scoped `stop_screen_recording` must
match that recording device.

See the Chrome + macOS and Chrome + Windows notification templates in
[`../examples/chrome_macos_notifications.yaml`](../examples/chrome_macos_notifications.yaml)
and [`../examples/chrome_windows_notifications.yaml`](../examples/chrome_windows_notifications.yaml).

Selectors support exact case-insensitive native `label`/`labels`, MCP `filter`,
`element_type`, `resource_id`, `class_contains`, `row_with`/`exclude_row_with`,
and explicit `prefer_resource_id`. `largest` selects a geometry viewport.
`native_tap` chooses the existing batch `tap_element` operation;
`stable_samples` requires repeated matching bounds. Unselected snapshot-local
resource IDs do not prevent stabilization; explicitly selected IDs still constrain
the match. Offscreen all-zero bounds use the existing bounded viewport search;
malformed bounds and ambiguous selectors remain errors. `require_focus` verifies
editor activation. `replace_recipe`/`verify_recipe` attach profile-specific
editing/verification sequences to `fill_field`.

The Orbit fixture now attaches stable `Semantics.identifier` values to fields,
buttons, task checkboxes and viewport containers. Rebuild/install the annotated
fixture before using the opt-in `targets-semantics.yaml`: `fixture_ready`
includes that requirement. `targets.yaml` retains the preceding selectors for
comparison until native evidence validates the experimental profile.
A Flutter `ValueKey` alone does not establish a native accessibility ID.

Selectors for these controls use the same `resource_id` across targets. The task
selectors do not constrain `element_type`, so a native checkbox/switch difference
does not change identification or rename the control. Email/password no longer
need native-label row relationships; avatar no longer needs an AL/Your account
fallback. Each viewport has `orbit-viewport`; `largest` chooses its geometry when
the widget-tree bridge propagates that identifier to descendants. Bounds remain
fresh, including nonzero origins and keyboard-shrunken layouts.

Single controls use merged Semantics so their identifier retains existing labels,
actions and checked/disabled state. Fields keep independent password-toggle
semantics. IconButton widget keys distinguish outer widget controls from internal
button widgets in the existing Windows snapshot; tests require exactly one
matching actionable node for the shared ID.

Text replacement, clipboard menu actions, focus confirmation and native-tap
stabilization remain target recipes/settings. Semantics identifiers do not remove
those native behavior differences. The runner has no Orbit labels or platform UI
branches. The selected `viewport` enables bounded down/up searches when missing
controls may be outside the current view. Widget semantics tests and dry-runs do
not establish native device parity; see the recorded verification results.

## Steps and semantics

| Step | Meaning |
| --- | --- |
| `select_device` | Exactly one available device matching ID/name/platform/state |
| `launch_app` | Shared launch: HTTP/HTTPS `app_id` uses `mobile_open_url`; native app IDs/paths use `mobile_launch_app`. Launch is not cold-reset |
| `tap`, `type_text`, `swipe` | Existing coordinate/text MCP calls |
| `tap_element`, `focus_field` | Fresh unique actionable match; optional native tap/focus proof |
| `fill_field` | Focus, type or run explicit replacement recipe, then verify |
| `gesture` | `from`/`to` fractions in target bounds, optional `duration_ms` |
| `scroll` | Measured viewport, `direction: up/down` |
| `assert_element` | At least one matching visible native node, bounded wait |
| `assert_absent` | No match in the current native tree; not proof about offscreen content |
| `assert_count` | Exact `count` after filtering; a capped query is an error |
| `assert_state` | `property: checked/focused/selected/enabled`, boolean `expected` |
| `assert_value` | Exact native field `expected`, including an empty string; explicit `value` takes priority over text/label |
| `list_elements` | Query evidence with the same selector rules |
| `if_element` | Exact single enabled match chooses `then`; zero chooses `else` |
| `run` | Expand a named recipe with `with` parameters |
| `reset_fields` | Clear only the runner's editing bookkeeping after navigation |
| `screenshot` | Decode MCP PNG to relative artifact `path` in v2 |
| `start_screen_recording`, `stop_screen_recording` | Existing MCP recording tools |
| `note_limitation` | Explicit unsupported assertion in an otherwise executed flow |
| `call_tool` | Named existing MCP tool with explicit `arguments`; `<selected>` substitutes the device ID; recording uses the dedicated steps |
| `wait` | Explicit bounded `ms` delay; prefer observed assertions for readiness |

Actions, value/state assertions and row anchors reject ambiguity. Disabled
controls and unknown state never become successful fallback matches. Null or
non-boolean native state is unknown. An explicit null/non-string field `value`
is unsupported; a matching label cannot override it. When `value` is absent,
the existing native text/description representation remains the fallback. MCP errors,
EOF, timeouts, Unsupported and capped/truncated queries fail the case. A tool
appearing in `tools/list` does not prove support on every device driver.

`launch_app` is the single YAML launch action in both versions. Supply a native
app identifier/executable path or an HTTP/HTTPS URL through `app_id`. The separate
`open_url` action has been removed; migrate its `url` value to `launch_app.app_id`.
The MCP navigation tool still requires a Chrome page; the old macOS/Safari
direct-driver examples are not equivalent MCP flows. Screenshots in v1 retain their explicit path
semantics; v2 paths stay under the case artifact directory.

## Evidence and adoption

Execute scenario UI steps through YAML and MCP. If a step cannot execute,
report its step ID, error and missing capability. Do not substitute an external
CDP script, manual UI action or app-code change to make a failing run pass.
Required driver initialization, such as automatic Flutter Web accessibility,
belongs inside MCP and must report initialization errors explicitly.

Each live case creates a fresh directory with `report.json`, `junit.xml`,
method/tool-only `mcp-calls.jsonl`, redacted `query-N.json`, requested screenshots
and `video.mp4` after recording stop. Input arguments/server stderr are not
persisted; text/asserted-value strings are redacted in query/error evidence.
`error_kind` distinguishes setup, assertion, unsupported, execution, interruption
and timeout failures. JUnit uses a failure for a failed assertion and an error
for setup/transport/execution failures. A case deadline or interruption also
records the active nested step as failed, even when execution was cancelled
before that step could finish.
Recording follows the YAML `start_screen_recording` and `stop_screen_recording`
steps. Environment-sourced input and `sensitive: true` do not interrupt recording,
block later recording starts, omit screenshots, or add a PARTIAL limitation.
An active recording is still stopped during case cleanup on completion, failure,
timeout or interruption. Text/asserted values remain redacted in query/error
evidence and input arguments are not persisted in the MCP trace. Screenshots and
video capture the application content visible between the YAML recording steps.

The CLI automatically burns step IDs and actions into a header above `video.mp4`;
input steps also show their resolved test input, including environment-sourced
values and steps marked `sensitive: true`. No YAML annotation steps are needed.
The application image is preserved below the header. `video-original.mp4` keeps
the unmodified recording. `video-steps.json` records step IDs/actions and runner
and video timestamps, without input values. Caption text files are temporary
and removed after rendering; the regular report/query/MCP trace redaction is
unchanged. Caption times are scaled to the encoded video duration because some
recorders drop frames instead of preserving wall-clock timestamps.

Step captions use the recorded result: green `[PASS]`, red `[FAIL]`, or yellow
`[INCOMPLETE]` when no completed result is available. These colors describe bot
execution; successful tap dispatch alone does not prove a screen transition.
A paginated summary at the end of the video lists all executed steps, their
results and intent notes. It also shows the case result and PASS/FAIL counts;
each page remains visible for 6–12 seconds. `video-summary.json` records those
pages and their timestamps. The original recording remains unchanged.

An optional step-level `note` describes intent without changing execution:

```yaml
- id: login-user
  note: Nhập tên đăng nhập của tài khoản kiểm thử
  fill_field:
    target: username
    text_from_env: TEST_USERNAME
```

Notes appear in `report.json`, video captions and the final summary. Recipe
children inherit the calling step's note when they have no note of their own.
Notes are literal text, not a second action or an MCP argument.

Video captions require `ffmpeg` with `drawtext` and a sibling `ffprobe` binary.
On macOS install `ffmpeg-full` (also used by
`device-bot --install-chrome-recording-tools`). `DEVICE_BOT_FFMPEG_PATH` can select
another compatible build. Caption generation errors fail the case with an
artifact error and retain the original recording instead of silently omitting
the captions. Library callers can render captions with `FlowRunner::annotate_video`
after `run` returns.

`PASS` means the declared checks executed successfully. `PARTIAL` retains
explicit limitations and is JUnit skipped. `DRY_RUN` means validation only;
it starts no subprocess and writes no artifacts. A live parse/setup/server-start
failure also produces a JUnit error report. The CLI exits nonzero on failure.

The Bash scenarios remain the baseline until each replacement is verified on
its target platform. Candidate YAML profiles passing validation do not establish
native parity. See the Orbit YAML results note for actual live evidence.
An opt-in GitLab template is at
`examples/flutter_app/automation_test/ci/mobile-yaml.gitlab-ci.yml`; it requires
an installed runner binary and an explicit external fixture setup script.

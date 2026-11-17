# Orbit UI flows through device-bot MCP

`it-01.yaml` through `it-09.yaml` are the executable native UI scenarios. The
Rust `robot-flow` client owns MCP transport, assertions, screenshots, recording,
deadlines and reports. The duplicate `it-*.sh` and shared `support/mcp.sh` have
been removed. Build/install and per-case fixture preparation remain outside the
flow, in [the CI wrapper](../ci/README.md).

`targets-semantics.yaml` is the default CI profile for this rebuilt fixture. It
uses shared Semantics identifiers for fields, buttons, task controls and
viewport, with native editing and IME recipes. `targets.yaml` retains the prior
selectors for comparison. `ValueKey` alone does not expose an Android/iOS native
identifier.

The recorded Simulator YAML regression completed with **4 PASS, 5 PARTIAL,
0 FAIL**, including all nine videos. Other platforms still require live YAML
verification. Historical shell results below do not establish YAML parity. See
[the YAML validation history](RESULTS_YAML_2026-11-04.md) and
[the Semantics correction report](../../../.codex-runs/semantics-yaml-fix-2026-11-04/report.md).

From the monorepo root, validate without touching a device:

```sh
cargo build --manifest-path server/Cargo.toml -p robot-flow
server/target/debug/robot-flow --dry-run \
  --profiles examples/flutter_app/automation_test/targets-semantics.yaml \
  --target ios-sim examples/flutter_app/automation_test/it-0*.yaml
```

For all nine cases on Android or Simulator, with a matching server and runner
already installed and the fixture built/installed from this checkout:

```sh
PLATFORM=ios-sim DEVICE_ID=<simulator-udid> APP_ID=com.example.flutter-myapp \
  bash examples/flutter_app/ci/run_yaml.sh
# Android: PLATFORM=android DEVICE_ID=<adb-serial> APP_ID=com.example.flutter_app
```

The wrapper cold-stops/prepares the fixture before every case. Windows uses
`ci/run_windows.sh` and the Rust `device-bot project windows-fixture start|stop`
helper for process setup/cleanup. `device-bot project ensure-report` supplies
missing setup-error JUnit. Both former support helpers have been deleted; use
a matching build that includes `robot-project` (see the CI guide). Keep the
native desktop/device unlocked and avoid manual input during the run. Simulator
IME checks require its software keyboard enabled.

`../project.yaml` lists these flows for `device-bot run`. It does not yet replace
the CI wrapper's platform-specific build/install/cold-stop preparation. Its
`before_each_flow` restarts the app and checks the login screen before each
Android or Simulator case. This resets the example's in-memory session; it is
not a general reset of persisted data or backend fixtures. On Windows the setup
flow checks readiness and the project-owned fixture starts a new process.
Validate the project and all nine cases without a device:

```sh
ORBIT_APP_PATH=placeholder server/target/debug/device-bot run examples/flutter_app \
  --platform ios-sim --confirm fixture_ready --dry-run
ORBIT_APP_PATH=placeholder server/target/debug/device-bot run examples/flutter_app \
  --platform android --artifact /path/to/app-debug.apk --confirm fixture_ready --dry-run
```

For a live Android run, remove `--dry-run` and select an available device with
`--device` when discovery is ambiguous. The project stages the APK and installs
it once through MCP before the case setup flows. The runner and MCP server must
share access to the staged file. A stopped emulator, Android Flutter bridge and
platform prerequisites still need their existing setup.

The repository's GitHub `QC automation tests` workflow validates this project
on Android and Simulator profiles. The optional `orbit_ios_live` manual input
runs it on a dedicated self-hosted `mobile-ios-sim` runner with `DEVICE_ID`
configured and uploads project evidence. It builds and installs the example
app first; it does not require the AstroTalk VPN or fixtures. The installed
Flutter/Xcode/FFmpeg and interactive Simulator requirements still apply.

For an individual case, including physical iOS, follow the external setup and
`--confirm fixture_ready` contract in [the YAML client guide](../../../server/docs/yaml-mcp-client.md).
Use the matching runner and server; physical iOS additionally requires a signed
fixture/XCTest runner and a phone-to-Mac bridge. The CI wrapper supports Android,
Simulator and Windows; it does not provision physical iOS.

`PASS` covers the scenario's scoped native UI assertions. `PARTIAL` means those
assertions succeeded with explicit limitations preserved in JSON/JUnit; it exits
zero and appears as skipped coverage in JUnit. Failures, missing reports and
invalid/missing video fail CI. Neither status replaces Flutter integration tests
or a QA review. Reports/screenshots/video can contain demo credentials.

| Case | Native UI coverage | Explicit limitations |
| --- | --- | --- |
| IT-01 | Cold launch, demo login, workspace and `1/3` | None within the scoped native UI flow |
| IT-02 | Empty/invalid validation, corrected email, successful login | None within the scoped native UI flow |
| IT-03 | Password visibility/value, IME Next focus and Done submission | None |
| IT-04 | Failed login, preserved input, retry | Busy state, disabled controls and duplicate requests |
| IT-05 | Password case/space rejection, normalized email | None within the scoped native UI flow |
| IT-06 | Reset field/validation/IME submission and demo login | Busy state and disabled controls |
| IT-07 | Account return after reset cancellation/completion | Direct internal GoRouter routing and `from` checks |
| IT-08 | Checkbox state, counters, tabs, account/profile | Progress indicator value and line-through style |
| IT-09 | Sign out removes account UI; login returns to Account | Direct internal GoRouter protected-route and `from` checks |

IT-10..12 are outside this native UI suite. OS deep links are outside the
fixture's scope. Shell validation records below are retained as historical
evidence for the former implementation.

## Validation on 2026-10-30

- `bash -n` passed for all nine entrypoints and the shared helper.
- All IT-01 through IT-09 have now been executed on Android `R58MA6QCSRJ`.
- IT-01, IT-02 and IT-05 passed their scoped native UI checks.
- IT-03, IT-04 and IT-06 through IT-09 completed their native UI checks and
  reported `PARTIAL` for the limitations listed above; these are not full
  replacements for their Flutter integration tests.
- IT-03, IT-06, IT-07 and IT-09 were executed in the follow-up run, along with
  IT-01 again against the latest shared helper. All five exited with code 0.
  Follow-up console logs: `/private/tmp/device-bot-remaining.K0mIEf/`.
- The new scripts/shared helper have not been revalidated on iOS Simulator.

The bullets above are the baseline before the shell scripts used element type, focus, checked state, and IME actions. A later run on the same Android device, `R58MA6QCSRJ`, covered the scripts that those tools can actually change:

- IT-03 exited 0 as `PASS`. IME Next moved focus to the password field and IME Done submitted the form.
- IT-06 exited 0 as `PARTIAL`. The reset screen has one `textField`, and IME Done submits a valid email. The remaining limitation is the 700 ms busy state and disabled controls.
- IT-08 exited 0 as `PARTIAL`. Checkbox `checked` was true for the initial task, false after clearing it, and true for all three completed tasks. `LinearProgressIndicator.value` and line-through remain in Flutter. The task list sits below the first viewport, so the checkbox query scrolls until the control is in the tree.
- IT-04, IT-07, and IT-09 were not changed. The short busy window and the GoRouter assertions are still outside these MCP tools.
- iOS Simulator was not part of this run.

## iOS Simulator validation on 2026-11-02

All IT-01 through IT-09 were executed through the global MCP binary with a
freshly built XCTest agent and app bundle `com.example.flutter-myapp`.
The initial run had IT-01 pass and IT-02 through IT-09 fail. See the
[baseline results](RESULTS_IOS_2026-11-02.md).

After fixing the iOS agent and helper, the latest results are **4 PASS,
5 PARTIAL, 0 FAIL**: IT-01/02/03/05 passed; IT-04/06/07/08/09 completed their UI
checks with the explicit limitations above. All nine exited zero. The full
regression was followed by an IT-05 rerun after the final text-payload whitespace
fix. See [the fixes, verification scope and evidence](FIX_IOS_2026-11-02.md).

## Physical iOS validation on 2026-11-02

The real-iOS runner now preserves text payload/focus and queries Flutter native
nodes with the same fixes as the Simulator. Physical-device IME also matches
semantic identifiers such as `Next:` when the visible label is localized.
The focused-input/whitespace/Next/Done/banner regression passed on an iPhone 15
Pro Max through the real runner's command dispatcher. The full shell suite over
the MCP LAN bridge remains unverified because the phone-to-Mac connection timed
out. See [the physical-device evidence and limitations](FIX_REAL_IOS_2026-11-02.md).

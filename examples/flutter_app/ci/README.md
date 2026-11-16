# Flutter app CI

GitHub and GitLab validate the app and run `automation_test/it-01.yaml` through
`it-09.yaml` sequentially through the Rust `robot-flow` runner. YAML is the source
of UI actions, assertions, screenshots, recording and coverage limitations. The
former Python suite runner and duplicate scenario shell files have been removed.
Shell scripts now handle environment setup and CI evidence checks.

`automation_test/ci/mobile-yaml.gitlab-ci.yml` remains a standalone include for
running one selected YAML from the monorepo root with an external setup script.
It is opt-in/manual; it is separate from the app-root CI's full nine-case suite.

These configurations assume the contents of `examples/flutter_app` are at the
CI repository root. In this monorepo the nested GitHub workflow is a template;
GitHub does not discover it automatically. When exporting the app, also copy
`packages/device_bot_flutter` and update its relative path in `pubspec.yaml`.

## Host and configuration

Validation needs Flutter 3.41.3, Bash, jq, FFmpeg and ffprobe. The example CI path
has no Python interpreter requirement. Native automation additionally needs a
matching `device-bot` and `robot-flow` build with its runtime assets. The default
mobile version is `0.1.24`; bootstrap also checks for the native `project
ensure-report` capability. Until a release includes `robot-project`, set
`DEVICE_BOT_BIN` to an updated build from this checkout, with its runtime assets.
An explicit binary is validated without being replaced. `ROBOT_FLOW_BIN`
defaults to the executable beside that binary; the suite rejects mismatched
binary versions before touching a device.

| Variable | Purpose |
| --- | --- |
| `PLATFORM` | `ios-sim` (default), `android`, or `windows` |
| `DEVICE_ID` | Required Simulator UDID, ADB serial, or `windows-host` |
| `APP_ID` | Required installed bundle/package; Windows wrapper sets the executable path |
| `DEVICE_BOT_VERSION` | Mobile bootstrap version, default `0.1.24` |
| `DEVICE_BOT_BIN` | Optional explicit server executable |
| `ROBOT_FLOW_BIN` | Optional matching Rust runner executable |
| `ORBIT_PROFILES` | Defaults to `automation_test/targets-semantics.yaml` for this rebuilt fixture |
| `SCENARIO_TIMEOUT` | Rust runner case deadline in seconds, default `600` |
| `RPC_TIMEOUT` | MCP request deadline in seconds, default `180` |
| `YAML_ARTIFACT_DIR` | Fresh output directory, default `artifacts/yaml` |

For GitLab, use a macOS shell runner tagged `mobile-macos` for validation and
`mobile-ios-sim` for automation. Set `DEVICE_ID` and
`APP_ID=com.example.flutter-myapp`. `MOBILE_TESTS_ENABLED` defaults to `true`;
`false` leaves a manual job. Android needs an appropriate `MOBILE_RUNNER_TAG`,
ADB, Java, Android SDK tooling and `APP_ID=com.example.flutter_app`.

For GitHub, validation runs on hosted Ubuntu. Mobile automation needs labels
`self-hosted`, `macOS`, `mobile-ios-sim` and the same native tools. Set Actions
variables `DEVICE_ID`, `APP_ID` and, when using a local build, `DEVICE_BOT_BIN`.
Dispatch with `mobile_tests=true`, or set
`MOBILE_TESTS_ENABLED=true` for push runs. Pull requests run validation.

Keep device runners assigned to trusted code. Resource groups and concurrency
serialize jobs within each service; use separate devices if both services can
run simultaneously. iOS Simulator needs Xcode and the software keyboard enabled
for native IME Next/Done assertions. Preparation rebuilds this checkout,
restarts only the selected Simulator and installs the fixture. Before each case
the wrapper cold-stops the fixture; on Simulator it warms XCTest before starting
the case's recording.

## Windows

Use GitLab tag `mobile-windows` or GitHub labels
`self-hosted`, `Windows`, `mobile-windows`. Install Flutter, Visual Studio desktop
C++ tooling, Git Bash, jq, FFmpeg and ffprobe. Run in an unlocked interactive
desktop session. Set `WINDOWS_DEVICE_BOT_BIN` to an absolute path to a Windows
build with desktop recording, with the matching `robot-flow.exe` beside it (or
set `ROBOT_FLOW_BIN`). The bootstrap validates this build without downloading a
mobile release. Set `WINDOWS_TESTS_ENABLED=true` or dispatch the manual job.

`ci/run_windows.sh` builds the fixture, sets its absolute `APP_ID`, then runs the
same nine YAML cases. `device-bot project windows-fixture start|stop` prepares a
fresh fixture process for each case through Win32 and closes only the
PID/start-time/executable it created. Whole-desktop
recording belongs to the native driver; CI verifies its resulting MP4. Windows
and Android YAML parity still require live runs on their respective hosts;
process fixtures only validate CI behavior.

## Evidence and checks

`ci/run_yaml.sh` compiles all nine cases before device actions. It excludes
unrelated YAML and drafts. Each case gets `artifacts/yaml/it-0N/`, including the
native `case-*/report.json`, JUnit, screenshots, MCP traces and video. CI checks
report status, video stream, positive duration and full video decoding. Missing
reports, invalid video or a nonzero runner exit fail the suite. Setup and CI
validation failures get supplemental error JUnit through `device-bot project
ensure-report`, including when fixture build/install fails before the suite
starts. This fallback requires a working device-bot binary. Preflight/warmup
reports are diagnostic and are
excluded from GitLab's published test results.

`PARTIAL` and its limitations stay in native JSON/JUnit and in `QA_REVIEW.md`.
QA review remains `PENDING`. The old `summary.json` and `preliminary.json` files
are replaced by native case reports. Both services retain evidence for 14 days
and upload after ordinary failures; a hard host shutdown can prevent upload.

Local checks from the app root:

```sh
bash ci/validate.sh
bash ci/tests/test_device_bot_install.sh
bash ci/tests/test_yaml_suite.sh
flutter pub get
flutter analyze
flutter test
```

The shell tests exercise bootstrap and CI contracts without network or device
access. Native helper tests run with `cargo test --manifest-path
server/Cargo.toml -p robot-project` and `cargo test --manifest-path
server/Cargo.toml -p device-bot --test project_cli` from the monorepo root.
`../project.yaml` lists the same nine flows for `device-bot run`; it still
requires fixture build/install and per-case cold-stop preparation. The CI
wrapper supplies those steps until project resources own their lifecycle.
MCP transport, timeout, cancellation and recording cleanup tests live in
`server/crates/robot-flow`; run `cargo test --manifest-path server/Cargo.toml -p
robot-flow` from the monorepo root. Some existing Rust transport test fixtures
still invoke Python; removal of those fixtures is separate from this example's
CI migration.

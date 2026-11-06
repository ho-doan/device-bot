# Flutter app CI

Adapted from [automation-test-mobile](https://gitlab.tpssoft.com/ho.doan/automation-test-mobile)
at commit `951e397168e23b35b85d07b620fa139dc497d9b5`.
Both platforms run validation followed by all shell scenario files directly in
`automation_test/*.sh`, sorted by filename and run sequentially on one device.
The suite discovers files on each run; scenario names and counts are not fixed.
Put shared helpers in subdirectories such as `automation_test/support/`, which
are not discovered as scenarios. An empty suite fails the automation job.

These are example configurations for a repository with the contents of
`examples/flutter_app` at its root. All job commands and artifact paths are
relative to that app root. The nested workflow is a template here; copy it to
the app repository's `.github/workflows/flutter-app.yml` to activate it.

When exporting this example, also provide its local `uia_windows` dependency:
copy `packages/uia_windows` from this repository into the app repository (for
example, at `packages/uia_windows`) and update `pubspec.yaml`'s dependency path
to match. The current `../../packages/uia_windows` path is for this monorepo.

## GitLab

Copy the example's `.gitlab-ci.yml` to the app repository root.
Use a macOS shell runner tagged `mobile-macos` for validation and
`mobile-ios-sim` for automation, as in the reference project. The runner needs
Flutter 3.41.3, Python 3.10+, Bash, jq, FFmpeg/ffprobe, curl and Xcode.

Set these project CI/CD variables:

| Variable | Value |
| --- | --- |
| `DEVICE_ID` | Simulator UDID; required |
| `APP_ID` | `com.example.flutter-myapp` for this iOS fixture; required |
| `MOBILE_TESTS_ENABLED` | Defaults to `true`; `false` makes automation manual |
| `PLATFORM` | Defaults to `ios-sim`; also supports `android` |
| `MOBILE_RUNNER_TAG` | Defaults to `mobile-ios-sim`; use the Android host tag for Android |
| `DEVICE_BOT_VERSION` | Defaults to `0.1.3`, matching the reference |
| `DEVICE_BOT_BIN` | Optional explicit binary path; validated without replacement |

Android runners also need ADB, Java and Android SDK tooling, a connected device,
and `APP_ID=com.example.flutter_app`. There is no image-based executor for the
native automation job.

## GitHub Actions

`.github/workflows/flutter-app.yml` validates pushes and pull requests in the
app repository. Validation uses hosted Ubuntu with Flutter
3.41.3. Native automation uses a macOS self-hosted runner with labels
`self-hosted`, `macOS`, `mobile-ios-sim`, with the same tools as the GitLab host.
Flutter must already be available on that host.

Set repository Actions variables `DEVICE_ID` and `APP_ID`. Use **Run workflow**
with `mobile_tests=true` to run the mobile job. Set the repository variable
`MOBILE_TESTS_ENABLED=true` to also run it on pushes. Pull requests run validation.

Keep device runners assigned to trusted code. GitLab resource groups and GitHub
concurrency serialize jobs within their own project/repository; give each service
a separate device/runner when both services can run concurrently. A runner
registered to the reference project is not automatically registered to this one.

## Windows desktop and multiple apps

Both example configurations include a separate Windows job. GitLab uses the
`mobile-windows` runner tag; GitHub uses `self-hosted`, `Windows`, `mobile-windows`.
Run the runner in an unlocked, interactive desktop session. Install Flutter,
Visual Studio's desktop C++ build tools, Git Bash, Python 3.10+ (`python3` on
PATH), jq, FFmpeg and ffprobe. A Windows service in session 0 cannot exercise
the interactive desktop.

Set `WINDOWS_DEVICE_BOT_BIN` to an absolute path to a device-bot Windows build
containing `robot-windows` desktop recording. Build that binary from the updated
device-bot source on Windows; the previously pinned `0.1.3` release is not
assumed to contain this feature. The Windows bootstrap checks the configured
binary and assets without downloading the older release. FFmpeg must be on
the runner's PATH, or set `DEVICE_BOT_FFMPEG_PATH` in the runner environment.

Use `WINDOWS_TESTS_ENABLED=true` for automatic Windows runs, or start the manual
GitLab job / GitHub workflow with `windows_tests=true`. The job builds the
example, sets `DEVICE_ID=windows-host` and `APP_ID` to the current build's
executable, then discovers the shell scenarios in `automation_test/*.sh`.

`mobile_start_screen_recording` captures the whole virtual Windows desktop,
including the pointer, using FFmpeg's
[`gdigrab` desktop input](https://ffmpeg.org/ffmpeg-devices.html#gdigrab).
UIA provides element discovery and native actions; it does not encode video.
The recorder is independent of the active process, so launching/selecting a
different app with `mobile_launch_app` changes the UI target while the same
video continues. It does not crop to the Flutter app or depend on its widget
bridge. Existing Orbit scenarios exercise Orbit; add scenarios to exercise
other apps or transitions between them.

The recorder waits for its first encoded frame before returning from start,
then sends FFmpeg `q` on stop to finalize the MP4. CI verifies the video stream
and duration with ffprobe and uploads it with the case evidence. Timeouts or
forced runner termination can leave incomplete video and are reported as errors.
Windows desktop capture and cross-app interaction still require a live Windows
verification run; process mocks only verify the recording lifecycle.

## Execution and evidence

The bootstrap reuses a healthy device-bot installation or installs the pinned
release and its assets using the reference project's pinned installer. An
explicit `DEVICE_BOT_BIN` override on macOS/Linux must pass its version and asset checks.
Build/install logs go into `artifacts/`. The preparation script rebuilds this
checkout and installs it on the selected device. On iOS Simulator it restarts
only the selected Simulator and builds using the host architecture, as in the
reference project's Xcode workaround.

Each case records video in the same MCP session used for UI actions. The helper
exports its evidence directory and writes `result.json`. The suite verifies each
video with ffprobe and writes `summary.json`, `junit.xml`, `QA_REVIEW.md` and
per-case `preliminary.json`. `PARTIAL` stays distinct from `PASS` and appears as
skipped coverage in JUnit; missing or invalid video fails the suite. QA remains
`PENDING` until reviewed.

Both services upload evidence even after failure and retain it for 14 days.
GitLab publishes the JUnit report; GitHub includes it in the downloadable artifact.
Hard runner shutdowns can prevent artifact upload.

Local validation, from the app root (`examples/flutter_app` in this checkout):

```sh
python3 ci/validate.py
python3 -m unittest discover -s ci/tests -v
flutter pub get
flutter analyze
flutter test
```

The Python tests use fake processes and test orchestration, cancellation,
installation and reporting. They do not exercise the app on a device.

Configuration references: [GitLab workflow rules](https://docs.gitlab.com/ci/yaml/workflow/),
[Flutter setup action](https://github.com/subosito/flutter-action).

# Orbit UI flows through device-bot MCP

`it-01.sh` through `it-09.sh` mirror the **observable native UI** portions of
`../integration_test/app_flows_test.dart`. They call the globally installed
`device-bot` through MCP stdio (`initialize`, `notifications/initialized`,
`tools/call`). `support/mcp.sh` shares transport, element queries, input, scroll,
cold-start and evidence handling. There is no Python or direct driver runner.

```sh
# Installed app, unlocked Android device.
PLATFORM=android DEVICE_ID=<adb-serial> bash examples/flutter_app/automation_test/it-02.sh

# Booted iOS Simulator with the installed example app and Xcode available.
PLATFORM=ios-sim DEVICE_ID=<simulator-udid> bash examples/flutter_app/automation_test/it-02.sh
```

Defaults: Android app `com.example.flutter_app`, iOS app
`com.example.flutter-myapp`. Override `APP_ID` for another installed build.
`APP_PATH=/absolute/path/to/Runner.app` or `APK_PATH=/absolute/path/to/app.apk`
installs the app through MCP before starting. `DEVICE_BOT_BIN` overrides the
server executable; `RPC_TIMEOUT` is a positive integer in seconds (default 180).
Dependencies: Bash, jq, base64 and the platform CLI (`adb` or `xcrun`).

Each invocation creates a fresh process/session and prints an artifact directory
containing MCP responses, numbered element queries, screenshots and any coverage
limitations. Demo credentials are included in these local artifacts. Only the
cold-stop precondition uses adb/simctl; app installation, launch, screenshots,
element inspection, gestures and typing go through MCP.

Input replacement uses a long press followed by the native **Select all** action
(case-insensitive, English UI). When the entire word is already selected, it
uses **Cut** and verifies the native field is empty before typing. If selection
is unavailable or incomplete, the script fails. iOS gestures use XCTest point
coordinates; Android gestures use screen pixels. Scripts search in both scroll
directions to reach controls after the keyboard or validation changes layout.
On Android, the password textbox is located beside the native Show/Hide password
action; the email textbox is the remaining input. iOS uses a typed `textField`
query with the native label. This avoids relying on Flutter
`ValueKey`s or Android hint attributes that MCP currently does not expose.
Swipes use the app ScrollView viewport so they do not hit the keyboard.
On iOS, taps wait for consecutive queries to report the same target bounds.
Text fields and editing menu items use native element taps; ordinary Flutter
controls use the verified visible bounds to avoid native auto-scroll surprises.
The iOS helper queries Orbit's Flutter checkboxes as native `switch` elements.
When replacing a masked demo password, the helper temporarily reveals it to
access text selection, verifies the replacement, and restores masking. The app
bar account avatar is exposed as the native `AL` button on Android.

| Script | Native UI coverage | Explicit limitations |
| --- | --- | --- |
| IT-01 | Cold launch, demo login, workspace and `1/3` | Router URI and Flutter exceptions |
| IT-02 | Empty/invalid validation, corrected email, successful login | Router URI |
| IT-03 | Show/hide password, preserved value, IME Next focuses password, IME Done submits | None. Show/Hide is the native stand-in for obscureText |
| IT-04 | Failed login, preserved input and successful retry | Does not claim to check the 700 ms loading state, disabled controls or duplicate request count |
| IT-05 | Reject password case/space variants; accept normalized email | Router URI |
| IT-06 | One text field, empty-email validation, IME Done submits the valid reset, demo login still works | The 700 ms busy state and disabled controls are not sampled |
| IT-07 | Account return destination survives reset cancellation and completion | Establishes return destination by signing out from Account; cannot cold-launch GoRouter `/account` or inspect `from` |
| IT-08 | Checkbox checked state, checklist counters, tabs, account data and profile navigation | LinearProgressIndicator.value and line-through style remain in Flutter |
| IT-09 | Sign out removes account UI; demo login returns to Account | Direct protected-route requests and `from` query checks remain in Flutter |

Scripts print `FAIL` and exit nonzero on an execution/assertion error. `PASS`
means the scoped UI checks succeeded. `PARTIAL` with exit zero means those UI
checks succeeded but the scenario has explicitly listed untested behaviors;
see `limitations.txt`. Neither marker replaces the Flutter integration suite.

Deep links are outside this example app's scope and are not counted as missing
coverage or a reason for `PARTIAL`. The IT-07/IT-09 limitations refer to the
original Flutter suite's internal GoRouter assertions, not OS deep links.

IT-10 through IT-12 are outside the requested `IT-0*` set. `it-01.yaml` remains a
reference for a future YAML-to-MCP client; these shell entrypoints do not invoke
`robot-flow`.

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

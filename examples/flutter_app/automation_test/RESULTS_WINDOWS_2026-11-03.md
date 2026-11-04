# Windows MCP validation — run 20261103-182930

All nine original `it-01.sh` through `it-09.sh` entrypoints ran sequentially
through **Git Bash and MCP stdio** against the Windows Flutter fixture.
Result: **4 PASS, 5 PARTIAL, 0 FAIL; every case exited 0**.

Environment: Windows 10, `windows-host`, Flutter 3.41.3 Debug fixture, the locally
built `device-bot` server, Git Bash and jq 1.8.1. The fixture starts the package
with `await UiaWindows.start()`; no scenario logic was added to the app.

## Results

| Case | Result | Observed UI behavior | Screenshot |
| --- | --- | --- | --- |
| IT-01 | PASS | Cold start, demo login, workspace, initial `1/3` | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-01/result.png) |
| IT-02 | PASS | Empty/invalid validation, full email replacement, successful login | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-02/result.png) |
| IT-03 | PASS | Password survives Show/Hide; IME Next changes focus; Done submits | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-03/result.png) |
| IT-04 | PARTIAL | Failed login preserves credentials; corrected password succeeds | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-04/result.png) |
| IT-05 | PASS | Rejects password case/space variants; accepts normalized email | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-05/result.png) |
| IT-06 | PARTIAL | One reset editor, validation, Done submission, subsequent demo login | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-06/result.png) |
| IT-07 | PARTIAL | Account return destination survives reset cancellation/completion | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-07/result.png) |
| IT-08 | PARTIAL | Checkbox states, `1/3 → 0/3 → 3/3`, tabs, profile and avatar | [checklist](../../../artifacts/automation/windows/20261103-182930/cases/it-08/completed-checklist.png) |
| IT-09 | PARTIAL | Sign out removes account UI; demo login returns to Account | [result](../../../artifacts/automation/windows/20261103-182930/cases/it-09/result.png) |

The existing coverage limitations were retained:

- IT-04: the 700 ms busy state, disabled controls and duplicate-submit count.
- IT-06: the 700 ms reset busy state and disabled controls.
- IT-07/09: direct internal GoRouter requests and `from` query assertions.
- IT-08: progress indicator value and line-through styling.

`PARTIAL` means the scoped UI assertions succeeded with these limits. No failed
assertions were suppressed, and the scenario entrypoints were not rewritten.
The existing `it-01.yaml` remains a future YAML-client reference.

## Implementation and fixes verified

- The shared helper accepts `PLATFORM=windows`, uses anonymous Bash coprocess
  pipes and jq binary output, and preserves the existing MCP protocol.
- A lifecycle helper creates a fresh Windows fixture per case, waits for its
  bridge, restores its window, and closes only the recorded PID/start time/path.
- Flutter traversal exports field messages, selectable account text and editor
  hit bounds. Editor bounds exclude validation text, labels and suffix buttons
  so native mouse drags select the complete old value.
- Field focus and the typed/replaced values are checked. Desktop duplicate
  “Sign out” controls are resolved with the profile's `sign-out-button` key.
- Flutter IME actions require the focused editor's configured action. Vertical
  swipes use Flutter touch gestures; horizontal selection remains native mouse
  input. These Flutter operations remain inside `packages/uia_windows/lib`.
- Native Windows helpers run without console windows. Foreground PID checks and
  activation that preserves existing editor focus prevent input going elsewhere.
- Literal text uses Unicode `SendInput`. The earlier `SendKeys` path was observed
  producing `Ỏbit123!` for `Orbit123!` under the host input method. The final run
  preserved literal case and spaces and passed IT-05.

## Evidence and regression checks

The [machine-readable summary](../../../artifacts/automation/windows/20261103-182930/summary.json)
and per-case console logs are under
`artifacts/automation/windows/20261103-182930/`. Each `cases/it-0N/` directory
contains copied MCP responses, numbered queries, screenshots and any
`limitations.txt`; evidence is retained in the workspace rather than only in
the original Git Bash `/tmp` folders.

Additional checks passed:

- Flutter package: 12 tests, clean analyzer, Windows Debug build.
- Rust adapter: 11 unit tests, clean Clippy and workspace formatting check.
- Opt-in live Rust test: native click, exact Unicode/metacharacter/space input
  (`+rust Ỏbit123! ^%{}[]  `), typed/key query, and native UIA fallback after
  temporarily unregistering the bridge.
- Bash syntax checks for the nine entrypoints and the shared helper.

All fixture/server processes created by the completed run and live smoke test
were closed. Android/iOS were not rerun as part of this Windows validation.

See [README.md](README.md#windows-through-git-bash) for the Git Bash commands.

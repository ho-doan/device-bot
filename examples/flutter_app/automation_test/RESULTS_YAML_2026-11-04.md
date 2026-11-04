# YAML-to-MCP validation on 2026-11-04

The new Rust client has no direct driver dependencies. This note records
verification of the client migration, not a replacement for the Bash regressions.

| Check | Result |
| --- | --- |
| Nine scenarios compiled/dry-run with each of four target profiles | 36 DRY_RUN; no device actions |
| Real installed device-bot 0.1.3, MCP discovery through YAML | PASS |
| iOS Simulator IT-01 | FAIL at `it-01-04` (`assert_element`): MCP tools/call timeout |
| Android live regression | NOT RUN; no ADB device attached |
| Windows live regression | NOT RUN; no Windows runtime on this host |
| Physical iOS live regression | NOT RUN; physical-device setup/bridge not validated |

The IT-01 attempt selected Simulator `8F30E506-B952-48EB-A1DB-8D61701356E0`,
started recording and launched the installed `com.example.flutter-myapp` fixture.
The first UI query did not return within the configured 60-second RPC deadline.
Recording cleanup also reported the poisoned transport; the client closed its
server process. A targeted process check found no remaining recordVideo process
for that device. This failure establishes no successful UI assertion.

Evidence from this attempt:
`/private/tmp/device-bot-yaml-live/case-1-34287-1793769748131241000/`.
The discovery-only probe used
`/private/tmp/device-bot-yaml-live/case-1-32915-1793769591275972000/`.
Temporary evidence may be removed during local cleanup.

The existing Bash files/results remain the baseline. IT-04/06/07/08/09 keep
their original limitations in YAML. Schema validation and fake-MCP tests must
not be reported as device coverage; full YAML adoption in live CI remains gated
on per-platform evidence.

## Continuation verification

The resumed client review added five regression tests and fixes for sensitive
recipe input, literal nested recipe parameters, native value precedence,
unknown null state and active-step reporting on a case deadline. All 30
robot-flow tests, formatting and Clippy with warnings denied pass. The 36 CLI
dry-runs were repeated successfully; CI setup-JUnit fallback checks also pass.

A new live IT-01 attempt against the installed device-bot reproduced the native
query timeout at `it-01-04`, using a 25-second RPC deadline. MCP initialization,
tool discovery, device selection, recording start and fixture launch succeeded.
The runner exited nonzero and saved JSON, JUnit and MCP call trace. Recording
cleanup reported the unusable transport; process inspection found no remaining
runner/server/recordVideo process for this attempt. No successful native UI
assertion, screenshot or completed video is claimed.

See the [continuation report](../../../.codex-runs/yaml-mcp-client-2026-11-04-resume/report.md)
and [live case report](../../../.codex-runs/yaml-mcp-client-2026-11-04-resume/live/case-1-66340-1793774518189523000/report.json).
These are local evidence artifacts. Android has no attached device; Windows and
physical iOS remain unverified. Live IT-02..09 and full migration acceptance are
still pending. The Bash baseline and opt-in CI gate remain in place.


## Semantics selector experiment

The fixture now exposes stable Semantics identifiers. The opt-in
`targets-semantics.yaml` identifies fields, buttons, avatar, task controls and
viewport by shared IDs. `targets.yaml` is retained for comparison. Native control
roles and text-editing recipes are preserved.

All 17 fixture widget tests and analyzer pass on Flutter 3.41.3. The added tests
check platform-themed Semantics action/state/identifier behavior and unique
Windows widget-tree bridge IDs. Nine scenarios compile for each target in both
profiles (36 experimental and 36 previous-profile dry-runs).

An arm64 Simulator build and installation succeeded. The native MCP probe still
failed at its first query, `email_id`, with a 25-second RPC timeout. No native
ID response or platform parity is claimed. See the
[Semantics experiment report](../../../.codex-runs/semantics-profiles-2026-11-04/report.md)
for the exact verified layer, successful build command and live failure evidence.


## MCP reinstall follow-up — latest result

Rebuilt/reinstalled device-bot 0.1.3 from `983dc40`, refreshed its XCTest assets,
rebuilt the Simulator runner and reinstalled the annotated fixture. Installation
manifest is valid and the installed binary hash matches the new build. Native
queries now return responses with a 110-second RPC deadline.

Native responses exposed two client defects, now covered by failing-before /
passing-after regressions: generated snapshot IDs were incorrectly treated as
stable IDs, and offscreen all-zero rectangles bypassed bounded viewport search.
All 33 robot-flow tests and Clippy pass after these client fixes.

The final native Semantics smoke is **PASS**: field/demo/avatar IDs, workspace,
viewport search and checked=true on visible task-1-checkbox are confirmed.
Screenshot evidence was saved and inspected. **IT-01 remains FAIL** only at
recording export: every UI check and screenshot passes, but the returned video
path has no file. An absolute artifact directory does not fix that gap. No
recording fallback or server/driver source change was made.

See the [MCP reinstall report](../../../.codex-runs/mcp-reinstall-2026-11-04/report.md)
for hashes, install checks, native traces, screenshots, JUnit and the remaining
coverage limits. Earlier timeout results above remain historical evidence.


## Recording diagnosis — latest IT-01 result

The missing-video cause is now identified: an old Simulator host recording from
12:22 remained active inside SimRenderServer. New recorder processes exited
with code 16 / Host recording is already in progress, while MCP still reported
started/saved because the driver discarded stderr and ignored the exit status.
After targeted runtime recovery, the old session finalized to a 45 MB video
lasting 8130 seconds. Cold-start readiness/early-stop behavior was also examined.

The plain MCP recording probe now succeeds. **IT-01 is PASS**, including all UI
assertions, both screenshots and MP4 export. The video is checked with ffprobe
and full ffmpeg decode. No production driver or tool-schema code changed in this
diagnosis; permanent lifecycle/error-reporting hardening remains separate work.

See the [recording diagnosis report](../../../.codex-runs/video-diagnosis-2026-11-04/report.md)
for the stderr, service/file-owner evidence, reproduction, recovery steps and
final video. Earlier IT-01 video failures above are historical results.


## Semantics profile correction — full Simulator rerun

The full `it-01.yaml`–`it-09.yaml` suite now completes with **4 PASS, 5 PARTIAL,
0 FAIL**, all exit 0, matching the post-fix shell baseline. IT-01/02/03/05 are
PASS; IT-04/06/07/08/09 keep their declared coverage limits and are PARTIAL.

The iOS Semantics profile narrows system editing menus to native menuItem
nodes and verifies field focus. The runner searches above and below its starting
viewport with the shell helper's bounded swipe pattern. YAML startup assertions
allow cold XCTest startup; regular searches remain bounded. The software
keyboard was shown through Device Hub before the final suite so native IME
Next/Done could be exercised. No app or native-driver code changed, and no
business assertion was removed.

All 36 robot-flow tests, Clippy with warnings denied, formatting and diff checks
pass. All nine MP4s are retained with ffprobe/full-decode checks. Post-suite health
shows recording_session=pass and no recording owners; agent_bridge is unknown
after the case servers exit. This does not establish other-platform parity.

See the [correction and full live report](../../../.codex-runs/semantics-yaml-fix-2026-11-04/report.md)
for per-case JSON/JUnit, videos, query traces, failure-before regressions, setup,
binary/profile hashes and retained limitations.

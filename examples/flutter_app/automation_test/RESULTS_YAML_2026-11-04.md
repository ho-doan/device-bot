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

# Automation edge-case audit

Reviewed 2026-11-15 against the checklist in `a.log`. Several groups already had
regression tests. The useful additions are tests across real failure boundaries,
rather than another copy of the same decision logic.

| Checklist group | Assessment and evidence |
| --- | --- |
| MCP disconnect during recording/replay | Existing stdio tests cover EOF, cancellation, late replies and timeouts. New [HTTP regression](../server/crates/robot-flow/tests/transport_failure.rs) commits a tap and drops its reply at step 5 of 20: the runner fails, sends the tap once, skips later actions, reconnects only for recording cleanup, and retains prior screenshot/video evidence and redacted traces. Automatic continuation of an ambiguously acknowledged action is not an at-most-once guarantee. |
| Device disappears or changes | Existing driver fixtures cover offline/locked devices, stale window identity and geometry, and failed capture setup. Chrome session tests also cover failed actions and independent cases. Physical USB removal, killing a real browser and closing real OS windows during input are not all exercised in CI. |
| Unstable selectors | Existing flow tests cover duplicates, disabled nodes, missing/truncated results, unstable geometry, positional selectors and native/Flutter fallback. Frame tests cover replaced documents; Windows input fixtures verify ownership and hit testing. Real platform overlay and DOM mutation scenarios still need the applicable native/browser integration environment. |
| Mid-flow replay failure | The new step-5/20 HTTP regression verifies stop-on-first-error, cleanup and retained evidence. Existing assertion tests check error classification and recording cleanup. A screenshot taken before losing a device is retained; this does not promise a new screenshot from a device that is already disconnected. |
| Concurrent actions and stale completions | Existing Flutter Cubit tests reject double Run and queued input after close/reconnect. New Chrome test boots two clients concurrently, requires distinct contexts on one browser connection and verifies closing one case preserves the other. This is session isolation, not a guarantee that two clients can safely interleave physical input on one device. |
| Invalid YAML | Existing tests cover unknown actions/variables/recipes, duplicate IDs and invalid positions. New document matrix includes syntax errors, unsupported versions, missing/empty steps, scalar steps, missing parameters and invalid nested branches, with no device tool execution on failure. |
| Export/import round trip | A real gap was confirmed: Flutter exported literal `${...}` text that Rust interpreted as a variable. Exporters now escape template openers. [Cross-language verification](../server/automation_test/verify_recording_contracts.py) replays Flutter-generated Chrome, Android and literal-text YAML through Rust and a stdio MCP fixture, comparing forwarded text with recorded values. CI runs this on Linux and Windows. Native file picker/save dialogs remain separate physical UI coverage. |
| Boundary text | New runner tests cover empty-input rejection, Vietnamese, emoji, combining characters, actual multiline/control characters, quotes/backslashes and 96 KB text, including redacted evidence. Escape regressions cover literal, nested and unfinished template openers. These establish serialization/transport behavior, not every native IME's capacity. |
| Reports and persistence | Existing tests cover concurrent projects, atomic rejection of corrupt imports, source removal and restart. New tests cover concurrent publication into one project and a missing published PNG without losing the report or remaining evidence. This is not a crash-during-write durability or large-history performance benchmark. |
| Timeouts and bounded retries | Existing flow and Flutter tests cover scenario/step deadlines, bounded inspection retries, reconnect limits, manual retry reset and close cancellation. Native network flapping remains an integration scenario. |
| Authentication | Existing tests cover missing/wrong credentials and endpoint scoping. New HTTP test requires bearer authentication on later POST/DELETE requests even when a valid session ID is supplied; an unauthorized DELETE must not close the legitimate session. Tokens are configured static bearer credentials: TTL expiry and live revocation are not implemented policies. Runner traces omit credentials/arguments and input evidence is redacted; this is not a claim about every external process's logs. |

## Confirmed fixes

- Report download race: a delayed Node fixture could configure an incognito
  context after YAML had clicked the download link. The same three-case test with
  a three-second helper startup delay failed 3/3 before and passed 3/3 after the
  fix. The full ten-case web suite passed. Browser-level auto-attach and a
  readiness handshake replace page polling; per-page setup finishes before
  script execution resumes. Commit: `6d5b894`.
- Literal recording replay: the YAML exported by Flutter failed with
  `unresolved variable literal_missing` before any device operation. `$${...}`
  now represents literal `${...}`; actual `${variable}` templates still resolve,
  and variable values remain opaque. The cross-language replay also checks
  preserved Unicode and text escapes.

## Repeatable checks

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path server/Cargo.toml --locked --workspace -- --test-threads=1
python3 -m unittest discover -s server/automation_test -p 'test_*.py' -v
node --test server/automation_test/file_chooser.test.cjs
```

From `server/flutter_app`, run:

```sh
LIVE_RECORDING_FIXTURE_DIR=/tmp/recording-contracts flutter test
python3 ../automation_test/verify_recording_contracts.py --fixtures /tmp/recording-contracts
```

The last command needs the current `server/target/debug/robot-flow` binary.
The protocol fixture never claims to type into a real Android/iOS editor.

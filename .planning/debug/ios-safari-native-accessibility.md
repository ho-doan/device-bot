---
status: verifying
trigger: "iOS Simulator must fall back to Safari when Chrome is unavailable and expose filtered native accessibility behavior equivalent to macOS."
created: 2026-09-28
updated: 2026-09-28
---

# iOS Safari Native Accessibility

## Symptoms

- Expected behavior: When Chrome is unavailable on an iOS Simulator, the example script launches Safari and native accessibility filtering works comparably to the macOS driver.
- Actual behavior: Chrome correctly fails and the script launches `com.apple.mobilesafari`, but the first `mobile_list_elements_on_screen` request times out before returning elements.
- Error messages: `XCTest runner timed out after 45 seconds; use filter and maxResults for web views`.
- Timeline: Reproduced in the current iPhone 18 Pro (iOS 27.0) Simulator runs; no earlier successful XCTest runner result is known.
- Reproduction: Boot simulator `F7DB2541-C54B-45BA-A51F-E7E52FC7BEB9`; run `DEVICE_ID=<UDID> bash server/examples/example.sh`; Chrome launch fails; Safari fallback launches; first filtered UI lookup times out.

## Current Focus

- bug_class: bohrbug (the invalid runner cache fails deterministically for the same request)
- hypothesis: The driver invokes `test-without-building` without ensuring its required runner products exist, while `ui_runner_status` treats an unrelated `.xctestrun` marker as readiness; a partial DerivedData cache therefore turns every first Safari query into a slow failure or timeout.
- reasoning_checkpoint:
    hypothesis: "`run_xctest` always starts `test-without-building`, but a cache with an `.xctestrun` marker and no host/test-runner apps cannot execute the test; the resulting Xcode setup failure consumes nearly the 45-second action budget."
    confirming_evidence:
      - "Before build, the exact filtered request took 40.60 seconds and Xcode named the absent `MobileMcpSimulatorRunnerHost.app`; no runner JSON was written."
      - "After `build-for-testing` into the same cache, the unchanged request succeeded in 29.94 seconds and the predicate itself took 0.45 seconds."
    falsification_test: "With an empty, temporary DerivedData path, a server using the fix must build the runner before testing and return a valid filtered lookup; a missing-product Xcode error or 45-second timeout would falsify it."
    fix_rationale: "Check for both products that `test-without-building` needs under the mutex and run `build-for-testing` when either is absent. The driver then cannot submit a query to a partial cache, and status exposes that same invariant."
    blind_spots: "Safari's current address-bar label did not match the selected text and returned an empty but valid result; this fix verifies runner availability, not the semantics of every Safari accessibility label."
    candidate_causes:
      - "code: `run_xctest` has no cache-initialization guard and `ui_runner_status` checks only an `.xctestrun` marker."
      - "config: the shared DerivedData cache was partial/stale, containing a marker but not the required app products."
      - "data: a Safari filter might legitimately match no elements, but it cannot explain the pre-dispatch missing-host error."
    and_gate: "yes — a partial cache exposes the failure only because the code unconditionally trusts it; the fix must both recognize cache completeness and build it when incomplete."
- hypothesis: The confirmed cache-completeness guard is race-safe only if `ensure_xctest_runner` executes after acquisition of the same `xctest_lock` that serializes `test-without-building`; the literal example must still perform Chrome failure → Safari fallback → prompt native lookup without the former runner timeout.
- test: Inspect the full `run_xctest` critical section and run the original example with `DEVICE_ID=F7DB2541-C54B-45BA-A51F-E7E52FC7BEB9` under a bounded host timeout.
- expecting: The mutex acquisition lexically precedes both runner initialization and Xcode test invocation, and the example prints an unavailable-Chrome failure followed by Safari launch and a returned native UI lookup (not the 45-second runner timeout).
- next_action: Read the complete XCTest execution and example-script paths, then run the bounded host-simulator reproduction.

## Evidence

- timestamp: 2026-09-28; `list_elements_filtered` passes `MOBILE_MCP_ELEMENT_FILTER` and `MOBILE_MCP_MAX_RESULTS` to the XCTest runner.
- timestamp: 2026-09-28; XCTest runner uses an `NSPredicate` for native matching on identifier, label, and value before materializing result elements.
- timestamp: 2026-09-28; `run_xctest` wraps `xcodebuild test-without-building` in a fixed 45-second timeout.
- timestamp: 2026-09-28; source inspection confirms that `list_elements_filtered` forwards both filter and result cap as `TEST_RUNNER_*` variables, and the test target applies the predicate before accessing `query.count` or element frames.
- timestamp: 2026-09-28; source inspection found no simulator `build-for-testing`/prewarm operation, while the runtime exposes a status warning that the XCTest cache must be prewarmed.
- timestamp: 2026-09-28; no debug knowledge base exists, so there is no matching prior-resolution candidate.
- timestamp: 2026-09-28; sandboxed `xcrun simctl list devices available` could not connect to CoreSimulatorService (connection refused and sandbox log permission denial), so simulator evidence must be collected outside the sandbox.
- timestamp: 2026-09-28; host CoreSimulator inspection confirmed that `F7DB2541-C54B-45BA-A51F-E7E52FC7BEB9` is booted on iOS 27.0 and has `com.apple.mobilesafari` installed.
- timestamp: 2026-09-28; the exact selective host XCTest request took 40.60 seconds, returned no JSON result, and Xcode reported that `MobileMcpSimulatorRunnerHost.app` was absent from the shared DerivedData `Build/Products/Debug-iphonesimulator` directory.
- timestamp: 2026-09-28; this direct runner error occurs before `testDispatch` can activate Safari or evaluate its `NSPredicate`, so the prior Safari-attachment hypothesis is eliminated.
- timestamp: 2026-09-28; `xcodebuild build-for-testing` completed successfully against the exact shared DerivedData path in 16.8 seconds, producing the simulator host and UI-test runner products required by `test-without-building`.
- timestamp: 2026-09-28; after that build, the unchanged `Address and search bar` request succeeded in 29.94 seconds, wrote `{\"ok\":true,\"elements\":[]}`, and Xcode measured only 0.45 seconds for its native predicate. The original timeout did not recur.
- timestamp: 2026-09-28; `cargo fmt --all --check` passed and `cargo test -p mobile-robot-ios-sim` passed all six tests, including `xctest_runner_cache_requires_both_app_products`.
- timestamp: 2026-09-28; with a fresh temporary `TMPDIR`, the patched public MCP flow initialized, launched `com.apple.mobilesafari`, and the original `filter: \"New Tab\", maxResults: 10` lookup returned one enabled, clickable native accessibility element. No 45-second timeout occurred.
- timestamp: 2026-09-28; `cargo test -p device-bot` passed all eight unit/integration tests. `git diff --check` passed; the focused driver diff only adds cache validation/build behavior and replaces the prior false-ready status message.
- timestamp: 2026-09-28; with only the `ensure_xctest_runner` call temporarily removed and a fresh temporary cache, MCP initialized and launched Safari but produced no response for the original filtered lookup before the driver's action deadline. This matches the original missing-runner timeout behavior.
- timestamp: 2026-09-28; after restoring the same two-line guard, a fresh public MCP session again returned the original filtered `New Tab` native element successfully.
- timestamp: 2026-09-28; SBFL skipped: no failing automated test plus per-test coverage spectrum exists for this host-simulator cache failure; diagnosis instead uses a controlled before/after host reproduction.
- timestamp: 2026-09-28; final `cargo fmt --all --check`, `cargo test -p mobile-robot-ios-sim` (6 tests), and `cargo clippy -p mobile-robot-ios-sim -- -D warnings` passed. Root-level `git diff --check` passed.

## Eliminated

- hypothesis: The first timeout is caused by Safari attachment or selector evaluation before the XCTest runner starts.
  evidence: The direct filtered request never launched the test because Xcode could not read the missing runner host app from DerivedData.
  timestamp: 2026-09-28

## Resolution

- root_cause: `run_xctest` trusted a partial shared XCTest DerivedData cache and never initialized it; `ui_runner_status` reported that partial cache ready from an `.xctestrun` marker even though `MobileMcpSimulatorRunnerHost.app` and the UI-test runner were absent.
- fix: `run_xctest` now verifies both XCTest app products while holding the shared-cache lock and invokes bounded `build-for-testing` when either is absent; `ui_runner_status` uses the same completeness rule. A regression test covers marker-only, partial, and complete cache states.
- oracle_type: specified — an XCTest cache is ready only when the runner products required by `test-without-building` are present.
- verification:
  target_test: { result: pass, suite: "cargo test -p mobile-robot-ios-sim", test: "xctest_runner_cache_requires_both_app_products" }
  mutation_check: { result: skipped, reason_if_skipped: "No Stryker configuration exists for this Rust workspace." }
  no_op_deletion: { result: pass, deletion_justified_by_rca: true, evidence: "Focused diff adds availability checks/build behavior; replaced status logic removes only the documented false-ready predicate." }
  adjacent_tests: { result: pass, suites_run: ["cargo test -p device-bot"] }
  revert_and_reconfirm: { result: pass, bug_returned_on_revert: true, fixed_on_reapply: true, evidence: "A fresh MCP lookup stalled with only the guard removed and returned the native New Tab element again after the exact guard was restored." }
  guardrail_verdict: accepted
- files_changed:
  - server/crates/robot-ios-sim/src/lib.rs

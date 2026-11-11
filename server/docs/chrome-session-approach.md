# Chrome session approach — paused handoff

Status: approach agreed; implementation paused at the user's request.
Date: 2026-11-11.

## Agreed behavior

Use the user's running Google Chrome browser. Create a separate incognito
browser context/window for each YAML case. Close only that case's context/window
when it ends, including assertion or execution failures. Keep the main Chrome
process, personal windows and unrelated tabs untouched. The next case creates a
new incognito context/window.

Do not spawn another Chrome process with a temporary user-data directory as an
implicit fallback. The user expects windows belonging to the normal Chrome app,
including normal window switching with Win/Command + backtick.

QC and YAML must reuse the same driver implementation. Transport/lifecycle may
differ, but QC must not have a separate browser-launch implementation.

## Connection and ownership design to implement

1. Discover/connect to the running Chrome browser through its supported remote
   debugging connection. Investigate Chrome DevTools MCP's auto-connect approach
   and local `DevToolsActivePort` discovery; do not assume that enabling a TCP
   debugging flag on the personal default profile works.
2. If debugging is not enabled or connection fails, return a clear setup error
   with the required action. Do not change Chrome settings automatically or
   launch a different browser to hide the error.
3. Use browser-level CDP `Target.createBrowserContext` and `Target.createTarget`
   to create the case-owned incognito context and window.
4. Restrict discovery/selection to targets owned by that context, avoiding
   personal tabs and DevTools frontend targets.
5. At case completion, dispose only the owned context with
   `Target.disposeBrowserContext`. Never terminate the shared browser PID.
6. Ensure cleanup works on success, assertion failure and transport failure.
   Define QC interactive-session ownership explicitly; do not accidentally
   dispose a manual QC window when an unrelated YAML case ends.

## Execution constraints

- Execute UI tests through YAML and MCP only.
- On failure, report the exact step, error and missing capability. No external
  CDP scripts/manual clicks/app-code changes as a workaround to pass a case.
- Flutter Web semantics activation is inside the MCP Chrome driver, not an
  external setup step.
- Keep credentials in the ignored local env file; do not include them in this
  document, code, commands printed as evidence or reports.
- Preserve explicit test limitations. AAT-1188 checks form rendering;
  AAT-1191 exercises input and Login. Do not conflate these or call PARTIAL a
  verified successful login.

## Current code and runtime state

- Existing implementation uses an isolated Chrome process/profile per MCP
  session by default; `DEVICE_BOT_CHROME_DEBUG_ADDRESS` explicitly selects attach
  mode. This is the earlier approach and is to be replaced for the agreed mode.
- Temporary profiles, child-process ownership, five-second CDP deadlines and
  cancellable recording capture were implemented. YAML cases AAT-1188/AAT-1191
  have a browser-boot step; the AstroTalk runner has an `auto` device option.
- A prototype macOS LaunchServices launcher was then added to the shared driver
  while diagnosing QC-only slowness. Its 11 Chrome unit tests passed, but it was
  not live-tested, packaged or deployed. It is not the agreed final approach;
  review/remove that prototype when implementing shared-browser contexts.
- Do not blindly reset the working tree: it contains the changes above and the
  untracked `astrotalk/` workspace with local scenario changes/test data.
- The last local QC bundle update predates the LaunchServices prototype and
  still uses the separate-process approach. It is under
  `artifacts/qc-release/smoke/Device Bot QC.app`.
- The user reported that Chrome started through QC still responds very slowly,
  although short-lived YAML/stdio tests had succeeded. The exact cause of that
  QC-only delay was not established. Do not claim it was fixed by a clean
  profile or native launch.
- Prior evidence is in `artifacts/chrome-case-lifecycle/RESULTS.md` and
  `artifacts/astrotalk-AAT-1188-retest/RESULTS.md`; it documents the earlier
  implementation, not validation of the new shared-browser approach.

## Resume checklist

1. Inspect the current diff, especially
   `server/crates/robot-chrome/src/startup.rs` and `src/lib.rs`.
2. Check whether the running normal Chrome exposes remote debugging; read only
   discovery metadata, never broad dumps of process environments/private logs.
3. Implement context/window ownership in the shared driver, retaining explicit
   timeouts and errors.
4. Add regression coverage for context isolation and disposal without closing
   personal tabs/browser, including failed cases.
5. Verify via YAML/MCP against the same HTTP MCP backend used by QC as well as
   stdio. CLI success alone is not evidence that the QC route performs well.
6. Verify video artifacts, cleanup and the next case getting a new context.
   Update the QC bundle only after those checks; identify which binary is
   actually running.

## Primary references

- [Chrome DevTools MCP auto-connect](https://developer.chrome.com/docs/devtools/agents/use-cases/auto-connect)
- [Chrome DevTools MCP advanced usage](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/advanced-usage.md)
- [Chrome remote debugging changes](https://developer.chrome.com/blog/remote-debugging-port)

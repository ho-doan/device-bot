## Unreleased

* Use `device-bot mcp` for the MCP server and `device-bot run <scenario.yaml>` for YAML execution.
* Make bare `device-bot` start the background report web app and open the browser; `device-bot start` reuses the healthy service or starts it when absent.
* Keep the web app running after the terminal closes, with history in `~/.device-bot/reports` and default port 8765.
* MCP clients that previously launched bare `device-bot` must add the `mcp` argument.

## 0.1.4 — Native tools

* Release device-bot, robot-flow and robot-report together for macOS arm64/x86_64 and Windows x86_64.
* Add persistent multi-project QA HTML reports with run history, failed-step details and original evidence downloads.
* Include production systemd/LaunchDaemon templates and reporter health/version commands.
* Ship YAML-to-MCP flows and corrected Flutter Semantics profiles; Simulator regression matches the shell baseline (4 PASS, 5 PARTIAL).
* Add bounded iOS health probes and Simulator recording ownership preflight.
* Verify all three CLI versions during installation while retaining support for older device-bot-only archives.

## 0.1.1

* Package versioned iOS Simulator and real-device XCTest assets with native releases.
* Resolve installed assets relative to the executable; add `--check-install`.
* Preserve ephemeral bridge ports when launching XCTest agents.
* Install and verify binary/assets together for reproducible CI setup.

## 0.0.1

* TODO: Describe initial release.

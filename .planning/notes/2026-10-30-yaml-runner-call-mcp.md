---
date: "2026-10-30 09:19"
promoted: false
---

có vẻ runner yaml đi sai hướng rồi. runner yaml để gọi mcp nhanh hơn, dễ hơn. chỉ là khai báo đẹp hơn.

note lại để implement sau

## Implementation context

The YAML runner should be a declarative MCP client for device-bot. YAML makes
authoring and running tool calls easier; the execution path must go through the
existing MCP server instead of instantiating platform drivers in the runner.

Current deviation: `server/crates/robot-flow/src/main.rs` constructs a
`RobotRegistry` with Chrome/macOS drivers, and `FlowRunner` calls `Robot`
directly. Adding Android registration there would continue the wrong direction.

Follow-up implementation criteria:

- Connect to device-bot through its supported MCP transport and complete the
  initialize/initialized lifecycle.
- Translate YAML device/app/input/element/screenshot/recording steps into
  existing MCP tool calls; use tools/list to validate required capabilities.
- Keep platform discovery, routing and driver ownership inside device-bot.
- Keep scenario orchestration (waits, assertions, retries and step reports) in
  the YAML runner, using MCP results and reporting MCP/tool errors clearly.
- Preserve a dry-run mode for validating YAML without device actions; clearly
  distinguish dry-run validation from an executed test.
- Do not invent unsupported MCP operations. Cold-stop/reset is a known gap;
  define an explicit prerequisite or add a server capability in future work.
- Use `examples/flutter_app/automation_test/it-01.yaml` as a reference scenario:
  login heading -> demo button -> workspace heading -> initial progress 1/3.
  GoRouter URI and Flutter exception assertions remain in the Flutter test.
- Verify that live YAML execution actually sends MCP tool calls and propagates
  tool failures; dry-run alone cannot establish this.

Deferred at the user's request. No runner implementation changes in this note.

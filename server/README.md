# Mobile MCP Rust server

The Rust server is intentionally independent of the Flutter plugin at the
repository root. It implements the MCP transport and device abstractions; the
Flutter companion can be added later through a local admin API without being
part of the MCP request path.

## Current milestone

Phase 0 is implemented:

- a Cargo workspace with protocol, server, core robot, and Android-driver crates;
- newline-delimited JSON-RPC 2.0 over stdio;
- MCP `initialize` / `notifications/initialized` lifecycle enforcement;
- a `tools/list` response with a valid, initially empty tool catalogue.

The supported MCP protocol version is `2025-11-25`.

The ownership and dependency rules that keep this workspace from accumulating
a central "god file" are documented in [architecture.md](docs/architecture.md).

## Run

```bash
cargo run -p mobile-mcp-server
```

The process accepts one JSON-RPC message per stdin line and writes one response
per stdout line. Diagnostic output is reserved for stderr.

For example:

```text
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

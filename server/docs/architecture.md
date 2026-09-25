# Architecture boundaries

The server is organized around directional dependencies so a transport,
protocol feature, or device driver can change without absorbing unrelated
responsibilities.

```text
mcp-server (binary + connection orchestration)
        │
        ├── mcp-protocol (wire types only)
        └── robot-core (device contracts only)
                │
                └── robot-android / future platform drivers
```

`mcp-protocol` never starts processes, owns connection state, or knows a
device. It owns JSON-RPC/MCP serde types and protocol constants.

`mcp-server` is split by concern:

- `stdio.rs`: line-oriented process I/O; no MCP routing decisions.
- `server.rs`: method routing and lifecycle-gated tool access; no I/O loops.
- `lifecycle.rs`: valid state transitions only.
- `response.rs`: JSON-RPC response construction only.
- `main.rs`: process entrypoint only.

`robot-core` owns portable `Robot`, device, point, and error contracts. It
must not refer to `adb`, `simctl`, HTTP, or MCP message types.

Each platform crate owns its external process commands, output parsers, and
driver-specific tests. `robot-android` may depend on `robot-core`, but no
platform driver may depend on `mcp-server`; tool-to-driver adaptation belongs
in narrowly scoped server handler modules added with each tool family.

When a tool catalogue is introduced, its definitions go in a dedicated
`mcp-server::tools` module and each family (device, app, screen, input,
crash) gets a separate handler module. The router stays a small method-to-
handler dispatch table rather than a business-logic container.

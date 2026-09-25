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
                └── robot-android / robot-ios-sim / future platform drivers
```

`mcp-protocol` never starts processes, owns connection state, or knows a
device. It owns JSON-RPC/MCP serde types and protocol constants.

`mcp-server` is split by concern:

- `stdio.rs`: line-oriented process I/O; no MCP routing decisions.
- `server.rs`: method routing and lifecycle-gated tool access; no I/O loops.
- `lifecycle.rs`: valid state transitions only.
- `response.rs`: JSON-RPC response construction only.
- `main.rs`: process entrypoint only.
- `http.rs`: Streamable HTTP session ownership, bearer-token validation, and
  JSON/SSE response encoding; it delegates every protocol request to `Server`.

`robot-core` owns portable `Robot`, device, point, and error contracts. It
must not refer to `adb`, `simctl`, HTTP, or MCP message types.

Each platform crate owns its external process commands, output parsers, and
driver-specific tests. `robot-android` may depend on `robot-core`, but no
platform driver may depend on `mcp-server`; tool-to-driver adaptation belongs
in narrowly scoped server handler modules added with each tool family.

`robot-ios-real` owns the `go-ios` command boundary and WDA/DeviceKit XML
normalization. It is intentionally independent of `robot-ios-sim`, whose
`simctl` lifecycle and recording state have different failure modes.

Tool definitions live in `mcp-server::tools`; each family (device, app,
screen, input, crash) gets its own handler module. The initial Android slice
uses `tools/device.rs`, `tools/app.rs`, `tools/navigation.rs`,
`tools/input.rs`, `tools/recording.rs`, `tools/screen.rs`, and `tools/ui.rs`, with generic request
parsing in `tools/arguments.rs`. The router stays a small method-to-handler
dispatch table rather than a business-logic container.

Temporary platform capability gaps are tracked in
[platform-parity.md](platform-parity.md). The final parity pass owns alignment
of shared MCP behavior rather than making a phase-specific handler a
cross-platform policy container.

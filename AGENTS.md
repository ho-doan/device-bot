# Working in Device Bot

Device Bot exposes native device automation through a Rust MCP server, a Flutter
QC client, and a YAML flow runner with persistent reports. Example apps are
automation targets; they are separate from the QC client.

## Code discovery

<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tools** (when available): `codegraph_explore` answers most code questions in one call — the relevant symbols' verbatim source plus the call paths between them. `codegraph_node` returns one symbol's source + callers, or reads a whole file with line numbers. If the tools are listed but deferred, load them by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` and `codegraph node <symbol-or-file>` print the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely — indexing is the user's decision.
<!-- CODEGRAPH_END -->

For Serena, read `initial_instructions` and activate this repository by absolute
path. Use symbol overviews, targeted symbol bodies, and references for areas
CodeGraph does not cover. Read configs and documentation directly as needed.

## Where to work

- `server/Cargo.toml` is the Rust workspace manifest; the repository root has no
  Cargo workspace. `server/crates/mcp-server` builds the `device-bot` executable.
- `server/crates/robot-core` owns portable driver contracts and routing;
  `robot-android`, `robot-ios-sim`, `robot-ios-real`, `robot-macos`,
  `robot-windows`, and `robot-chrome` implement platform behavior.
- `server/crates/robot-flow` compiles and runs YAML scenarios; `robot-report`
  stores evidence and serves the report UI; `robot-stream` owns shared Live
  session infrastructure.
- `server/flutter_app` is the QC Flutter client. `packages/device_bot_flutter`
  supplies optional widget-tree discovery inside target Flutter apps.
- `examples/` contains target fixtures. `scripts/` and `.github/workflows/`
  own packaging, release, and CI automation.

Read `server/docs/architecture.md` before changing Rust module ownership or
public contracts. Read `server/flutter_app/README.md` before QC changes,
`docs/architecture-live.md` for Live changes, and
`server/docs/yaml-mcp-client.md` for flow syntax or exporter changes.
Use `docs/repository-overview.md` for the full application and helper map.

## Implementation conventions

### Rust

- For Rust changes, use [rust-best-practices](.agents/skills/rust-best-practices/SKILL.md)
  with Device Bot's [Rust architecture rules](server/docs/architecture.md#rust-implementation-and-review).
- For Rust review, use [rust-review](.agents/skills/rust-review/SKILL.md) and
  check dependency direction, native resource ownership, and compatibility of
  public contracts in the requested scope.
- For ports into Rust, use [rust-port](.agents/skills/rust-port/SKILL.md) and
  carry the existing behavior tests into the owning crate.

Use this repository's Cargo commands under Build and validation. The skills
are vendored from [Beautiful Rust](https://github.com/morganlinton/BeautifulRust/tree/b9718436d77ee7bcf17cdcb6aa27628feb95d97a).

### Project conventions

- Keep `robot-core` independent of MCP/HTTP types and platform commands. Drivers
  own platform processes and parsers; MCP tool adaptation belongs in
  `mcp-server/src/tools/`. Preserve the transport/routing/lifecycle boundaries
  described in the architecture document.
- QC uses feature Cubits for asynchronous state and Dio for HTTP. Keep widgets
  focused on rendering resources and gestures; render state through
  BlocBuilder/BlocConsumer. Preserve generation checks, request cancellation,
  and resource cleanup when changing connection, inspector, or Live behavior.
- Edit Dart source models, regenerate with `dart run build_runner build` from
  the owning Flutter package, and commit the generated `.g.dart` changes.
  CI checks generated output in QC and the bridge package.
- For flow execution, keep shared tool payload construction in
  `robot-flow/src/runner/arguments.rs`. Preserve typed error classification,
  persisted report status strings, and literal-text escaping across QC export
  and Rust replay; details are in `server/docs/architecture.md`.

## Build and validation

Run Rust commands from the repository root with the manifest path, or from
`server/` without it:

```sh
cargo build --manifest-path server/Cargo.toml --locked -p device-bot -p robot-flow -p robot-report
cargo test --manifest-path server/Cargo.toml --workspace --locked
cargo clippy --manifest-path server/Cargo.toml --workspace --all-targets -- -D warnings
cargo fmt --manifest-path server/Cargo.toml --all -- --check
```

Start with tests for the changed crate using `-p <package>`. After changing
public driver contracts, also run
`cargo check --manifest-path server/Cargo.toml --workspace --all-targets`.

In each changed Flutter project (`server/flutter_app`,
`packages/device_bot_flutter`, or `examples/flutter_app`):

```sh
flutter pub get
flutter analyze
flutter test
```

For QC browser streaming changes, also run from `server/flutter_app`:

```sh
flutter test --platform chrome test/browser/streaming_adapter_test.dart
flutter build web --release --base-href /qc/
```

Additional checks by scope, from the repository root:

```sh
node --test server/crates/robot-chrome/tests/dom-elements.test.cjs
node --test server/crates/robot-chrome/tests/live.test.cjs
python3 scripts/test_package_release.py
python3 -m unittest discover -s server/automation_test -p 'test_*.py' -v
```

For full QC web replay, follow `server/automation_test/README.md` and
`.github/workflows/automation-tests.yml`; it needs a built backend, QC web
assets, Chrome, and an interactive or virtual display.

## Runtime and evidence

- Use `device-bot mcp` for stdio MCP; a bare installed `device-bot` opens reports.
  Use `mcp --listen HOST:PORT` for HTTP. Consult `server/README.md` for platform
  setup, authentication, signing, and development/release configuration.
- Rebuild and relaunch QC after Flutter/plugin changes; rebuilding Rust alone
  does not update the running UI.
- Validate scenarios with `device-bot run --dry-run flow.yaml` before replay.
  Distinguish schema validation, mocked tests, live execution, and reviewed
  screenshot/video evidence when reporting results. State unavailable platform
  checks explicitly; device discovery alone does not prove UI control.

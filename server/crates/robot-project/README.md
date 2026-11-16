# robot-project

Minimal project orchestration for the installed `device-bot` CLI. A project lists
existing YAML flows and project-local environment defaults; each flow executes
through the existing `robot-flow` binary and MCP backend.

```sh
cargo build --manifest-path server/Cargo.toml -p device-bot -p robot-flow
server/target/debug/device-bot init /tmp/my-project
server/target/debug/device-bot run /tmp/my-project --dry-run
server/target/debug/device-bot run /tmp/my-project
```

Initialization creates `project.yaml`, `.env`, `.gitignore`, `README.md` and
`flows/smoke.yaml`. The destination must be new or empty. The smoke checks the
real runner/MCP handshake without device actions; it is not application coverage.

```yaml
version: 1
kind: project
name: my-project
env_file: .env
flows:
  - flows/login.yaml
  - flows/chat.yaml
# Optional Flutter desktop fixture, prepared before each flow on Windows:
# windows_fixture: '${env:APP_ID}'
```

Use a directory or explicit manifest path with `device-bot run`. Options such as
`--profiles targets.yaml --target android --device ID --app-id APP` are passed
to the existing flow runner. Manifest references and relative CLI paths resolve
from the project directory. `.env` values are defaults: existing process env wins.
The parent environment is never mutated and dotenv values are not printed.

Flows run sequentially and stop at the first failure. Each run gets a fresh
`artifacts/project/run-*/case-NNN/` directory with native JSON/JUnit evidence;
`--artifacts DIR` changes the project artifact base. Errors outside the flow
runner get supplemental setup JUnit. Cancellation attempts flow cleanup before
stopping a project-owned Windows fixture.

The same crate owns the native CI helpers:

```sh
device-bot project ensure-report artifacts/yaml/job-error
device-bot project windows-fixture start /path/to/flutter_app.exe state.json
device-bot project windows-fixture stop /path/to/flutter_app.exe state.json
```

The reporter preserves existing direct case reports and ignores nested
preflight/warmup reports. Windows fixture setup uses Win32, not PowerShell:
it rejects an existing fixture, waits for its window/widget bridge and restores
foreground; cleanup checks PID, creation time and executable before closing it.
Only the process represented by its state file can be stopped.

This first version does not own a persistent multi-target session or project-wide
recording. Those resources need project lifetime so QC can open all targets
first, run shared flows and record the desktop once across the complete run.

Tests:

```sh
cargo test --manifest-path server/Cargo.toml -p robot-project
cargo build --manifest-path server/Cargo.toml -p device-bot -p robot-flow
cargo test --manifest-path server/Cargo.toml -p device-bot --test project_cli
cargo check --manifest-path server/Cargo.toml -p robot-project --target x86_64-pc-windows-msvc
```

Cross-compilation verifies Windows API/types only; live window/bridge behavior
requires an interactive Windows host with the Flutter fixture installed.

## Named project plans (schema only)

Version 1 also accepts named hosts and resources and a shared flow with named
bindings. Loading validates this plan but does **not** execute it. The current
runner rejects named plans before creating artifacts; ordinary path-only flows
continue to run as before. Runtime capability checks, preparation, ownership,
remote connections and recording remain future work.

```yaml
version: 1
kind: project
name: multi-target
env_file: .env
hosts:
  - name: mac
    platform: macos
    binding: {kind: local}
  - name: win
    platform: windows
    binding: {kind: mcp, endpoint: '${env:WINDOWS_MCP}'}
resources:
  - name: browser
    kind: chrome
    host: mac
    display: {host: mac}
    prepare: {kind: launch, url: 'https://example.com'}
    timeout_ms: 30000
    requires: [discovery, input]
  - name: desktop-app
    kind: windows
    host: win
    display: {host: mac, via: remote_desktop}
    selector: stable-app-identifier
    prepare: {kind: attach}
    timeout_ms: 30000
flows:
  - path: flows/shared.yaml
    profiles: profiles.yaml
    bindings:
      - {target: browser, profile: web}
      - {target: desktop-app, profile: windows}
```

Host platforms are `macos`, `windows`, and `linux`; binding is explicitly
`local` or `mcp` with an endpoint. Resource kinds are `android`,
`ios_simulator`, `ios_real`, `chrome`, `windows`, and `macos`. iOS and macOS
resources require a macOS execution host; Windows resources require Windows.
The display host is separate from the execution host; it must be macOS or
Windows, and a different host requires `via: mirror` or `remote_desktop`.
Declaring a display does not start a mirror or recorder. MCP endpoints may
reference `${env:NAME}`; scoped resolution is available through `Project::resolve`
without changing the process environment. Do not put secrets directly in YAML.

`prepare: {kind: attach}` declares use of an existing resource. `launch` declares
project-created work and needs `url` for Chrome or `app_id` for native targets;
`boot: true` is permitted only for Android and iOS Simulator. The optional
`selector` is a stable device ID or uniquely identifying name for driver discovery, not a raw query, command, script, or transient Chrome target ID.
`timeout_ms` must be positive. Supported capability names are `discovery`,
`launch`, `input`, `screenshot`, and `elements`; listing them does not prove
availability. A binding `profile` names a target in the flow runner profiles file, supplied by the bound flow’s project-root-relative `profiles` path (equivalent to `--profiles` and `--target`). The file must exist; target existence and profile contents remain runtime checks. Bindings select resources and optional flow profiles. Unknown
fields, duplicate names or bindings, invalid references and incompatible hosts
are rejected. Future actor, recorder and observer fields are not yet accepted.

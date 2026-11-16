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

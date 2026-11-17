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

A flow entry can also name a directory. The project selects its immediate
`.yaml` and `.yml` files in filename order; subdirectories are not traversed.
An empty directory is an error. Bound entries apply their profiles and bindings
to each selected file. Directory selection does not reset application state
between cases: prepare an independent fixture before each case as required.

```yaml
flows:
  - flows/scenarios
```

Path-only projects can store flow profiles and target defaults under platform
names, so the caller selects the target with one option:

```yaml
version: 1
kind: project
name: app-tests
flows: [flows/scenarios]
platforms:
  web:
    profiles: profiles/web.yaml
    target: chrome
    device: 'chrome:browser'
    app_id: '${env:APP_URL}'
  android:
    profiles: profiles/native.yaml
    device: '${env:ANDROID_DEVICE_ID}'
    app_id: com.example.app
```

Run `device-bot run . --platform web`. `target` defaults to the platform
name; `profiles` is relative to the project root. `device` and `app_id` are
optional, support `${env:NAME}`, and can be overridden with `--device` and
`--app-id`. `--profiles` and `--target` cannot override a selected platform.
Each project with `platforms` requires `--platform`; named resource projects do
not yet support platform defaults. These settings select an existing profile and
target; they do not download an artifact, install an application or reset its
state.

Projects may declare a per-case fixture preparation command. This is useful when
the application must be reset between cases, as launching it does not itself
prove a reset. The command runs once before each live case; it is skipped during
`--dry-run`. A failed, timed-out or interrupted preparation stops the suite and
produces setup JUnit evidence. No shell is inserted and the executable receives
each `args` entry as a separate argument. A project must contain trusted commands;
the project runner does not sandbox them or guarantee cleanup of their children.

```yaml
before_each:
  command: python3
  args: [scripts/reset_fixture.py]
  timeout_seconds: 180
```

The working directory is the project root. The command receives
`DEVICE_BOT_CASE_NAME`, `DEVICE_BOT_CASE_PATH`, `DEVICE_BOT_CASE_DIR` and, when
selected, `DEVICE_BOT_PLATFORM`, plus the project environment. Its arguments may
use `${env:NAME}`. The command must perform and verify the application-specific
reset; declaring this hook does not itself reset a device or test account. It is
currently available only for path-only projects.

`device-bot run . --platform web --artifact <file-or-link>` passes the supplied
value to `before_each` as `DEVICE_BOT_ARTIFACT`. The option requires a hook so
an artifact is never silently ignored. The hook is responsible for resolving
relative paths, downloading links, validating the artifact and preparing the
app; the project runner does not yet perform those actions. Protect URLs with
credentials in the hook's output as well as in CI logs. A dry-run checks the
configuration but neither invokes the hook nor validates the artifact.

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

Named projects use project-lifetime MCP sessions; project-wide desktop recording
is not yet implemented. The runtime is also exposed as `robot_project::session`
with an injectable session factory for embedding and testing.

Tests:

```sh
cargo test --manifest-path server/Cargo.toml -p robot-project
cargo build --manifest-path server/Cargo.toml -p device-bot -p robot-flow
cargo test --manifest-path server/Cargo.toml -p device-bot --test project_cli
cargo check --manifest-path server/Cargo.toml -p robot-project --target x86_64-pc-windows-msvc
```

Cross-compilation verifies Windows API/types only; live window/bridge behavior
requires an interactive Windows host with the Flutter fixture installed.

## Named project sessions

Version 1 accepts named hosts and resources and runs a shared flow once per
binding. Each resource owns an MCP connection for the entire project. The
runtime validates every case and available tool schema before preparing targets,
then routes stable project aliases to the selected device in each connection.
A Chrome `launch` creates an owned browser context and opens its URL; cleanup
closes the project's MCP sessions. Ordinary path-only projects keep their prior
runner path.

For named projects the supported CLI options are `--dry-run`, `--artifacts`,
`--confirm`, `--rpc-timeout` and `--case-timeout`. Profile selection is declared
in each flow binding. The bound resource overrides `profile.device.id` and
`profile.device.platform` so existing `select_device: ${device}` flows select the alias.
Each completed case is saved and imported into report history before cleanup.

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
    binding: {kind: mcp, endpoint: '${env:WINDOWS_MCP}', allow_remote: true, token_env: WINDOWS_TOKEN}
resources:
  - name: browser
    kind: chrome
    host: mac
    display: {host: mac}
    prepare: {kind: launch, url: 'https://example.com'}
    timeout_ms: 30000
    requires: [discovery]
  - name: desktop-app
    kind: windows
    host: win
    display: {host: win}
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

Host and resource names are portable identifiers: 1–64 ASCII letters, digits,
underscores, or hyphens, beginning with a letter or digit. Windows reserved
device names and names differing only by case are rejected.

Host platforms are `macos`, `windows`, and `linux`; binding is explicitly
`local` or `mcp` with an endpoint. Resource kinds are `android`,
`ios_simulator`, `ios_real`, `chrome`, `windows`, and `macos`. iOS and macOS
resources require a macOS execution host; Windows resources require Windows.
The display host is separate from the execution host; it must be macOS or
Windows, and a different host requires `via: mirror` or `remote_desktop`.
Declaring a display does not start a mirror or recorder. MCP endpoints may
reference `${env:NAME}`; scoped resolution is available through `Project::resolve`
without changing the process environment. Do not put secrets directly in YAML.

`prepare: {kind: attach}` selects an already available resource. Chrome
`launch` creates a project-owned context and navigates it to `url`. Native
launch/boot is rejected before setup: the current adapter cannot prove process
ownership and safely stop only the app it created. A future native adapter must
return a verifiable process identity and a corresponding owned cleanup handle. The optional
`selector` is a stable device ID or uniquely identifying name for driver discovery, not a raw query, command, script, or transient Chrome target ID.
`timeout_ms` must be positive. The runtime verifies the discovered device's
host OS and evaluates each `requires` entry using the read-only
`mobile_get_capabilities` report. Only `available` satisfies a requirement;
`blocked`, `unsupported`, and `unknown` stop setup. Discovery alone does not
prove UI control, and drivers may conservatively report readiness as unknown. A binding `profile` names a target in the flow runner profiles file, supplied by the bound flow’s project-root-relative `profiles` path (equivalent to `--profiles` and `--target`). The profile target and contents are checked before resource preparation. Bindings select resources and optional flow profiles. Unknown
fields, duplicate names or bindings, invalid references and incompatible hosts
are rejected. Future actor, recorder and observer fields are not yet accepted.

MCP hosts can opt into plaintext LAN transport explicitly, for example
`binding: {kind: mcp, endpoint: '${env:REMOTE_URL}', allow_remote: true, token_env: REMOTE_TOKEN}`.
The token is read only from the named environment variable; ambient local
credentials are never forwarded to the endpoint. HTTP has no transport encryption;
use it only on a trusted network. Local owned stdio servers default their iOS
bridge binds to ephemeral loopback ports so independent sessions do not conflict;
explicit user-provided bind values are preserved and must not collide.

Remote display/mirroring is declared but rejected at runtime until an adapter
can verify and own it. The capability report verifies reported host OS and available readiness probes;
unknown capability or permission states are not treated as available. Input is serialized
within one project run; cross-process and cross-machine input leasing is not yet
implemented. Do not run concurrent projects against the same native desktop.
Cancellation and setup failure close owned sessions; dropping the embedding
future schedules best-effort asynchronous cleanup while its Tokio runtime lives.

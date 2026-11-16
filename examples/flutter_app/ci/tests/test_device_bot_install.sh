#!/usr/bin/env bash
# shellcheck disable=SC2016
# Bootstrap contracts, without network access or changing the host installation.
set -euo pipefail
# Shell snippets below are evaluated only in fixture child processes.
script="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/ensure_device_bot.sh"
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/bin" "$root/install dir"
cat > "$root/healthy" <<'SH'
#!/bin/sh
case "$1" in --version) echo 0.1.24;; --check-install) exit 0;; *) exit 9;; esac
SH
chmod +x "$root/healthy"
cat > "$root/bin/curl" <<'SH'
#!/bin/sh
touch "$TEST_ROOT/downloaded"
while [ "$1" != --output ]; do shift; done
shift
cp "$TEST_ROOT/installer" "$1"
SH
chmod +x "$root/bin/curl"
reset_fixture() {
  rm -f "$root/downloaded" "$root/install dir/device-bot" "$root/bin/uname"
  cat > "$root/installer" <<'SH'
#!/bin/sh
cp "$TEST_ROOT/healthy" "$DEVICE_BOT_INSTALL_DIR/device-bot"
SH
}
bootstrap() {
  env -u DEVICE_BOT_BIN -u DEVICE_BOT_ASSET_DIR \
    PATH="$root/bin:/usr/bin:/bin" TEST_ROOT="$root" \
    DEVICE_BOT_INSTALL_DIR="$root/install dir" DEVICE_BOT_VERSION=0.1.24 \
    "$@" bash -c 'source "$1"; "$DEVICE_BOT_BIN" --check-install' bootstrap "$script" \
    > "$root/stdout" 2> "$root/stderr"
}
expect_success() { bootstrap "$@" || { cat "$root/stderr" >&2; exit 1; }; }
expect_failure() { if bootstrap "$@"; then echo 'Expected bootstrap to fail' >&2; exit 1; fi; }
reset_fixture
cp "$root/healthy" "$root/install dir/device-bot"
expect_success
[[ ! -f "$root/downloaded" ]]
reset_fixture
expect_success
[[ -f "$root/downloaded" ]]
reset_fixture
sed 's/--check-install) exit 0/--check-install) exit 1/' "$root/healthy" > "$root/install dir/device-bot"
chmod +x "$root/install dir/device-bot"
expect_success
[[ -f "$root/downloaded" ]]
reset_fixture
sed 's/0.1.24/0.1.1/' "$root/healthy" > "$root/install dir/device-bot"
chmod +x "$root/install dir/device-bot"
expect_success
[[ -f "$root/downloaded" ]]
reset_fixture
cp "$root/healthy" "$root/custom bot"
expect_success DEVICE_BOT_BIN="$root/custom bot"
[[ ! -f "$root/downloaded" ]]
expect_failure DEVICE_BOT_BIN="$root/missing"
[[ ! -f "$root/downloaded" ]]
printf '#!/bin/sh\necho MINGW64_NT\n' > "$root/bin/uname"
chmod +x "$root/bin/uname"
expect_success DEVICE_BOT_BIN="$root/custom bot"
[[ ! -f "$root/downloaded" ]]
expect_failure
[[ ! -f "$root/downloaded" ]]
reset_fixture
printf '#!/bin/sh\nexit 22\n' > "$root/bin/curl"
expect_failure
reset_fixture
printf '#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nshift\nprintf "exit 1\\n" > "$1"\n' > "$root/bin/curl"
expect_failure
printf '#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nshift\nprintf "exit 0\\n" > "$1"\n' > "$root/bin/curl"
expect_failure
printf 'Bootstrap: 11 checks passed (no network/device access).\n'

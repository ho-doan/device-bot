#!/usr/bin/env bash
# Source this script so PATH and DEVICE_BOT_BIN reach subsequent CI commands.
set -euo pipefail

case "$(uname -s)" in
  MINGW*|MSYS*)
    # Desktop recording needs a build containing the Windows recorder.
    # Use the runner's verified build rather than an older published release.
    : "${DEVICE_BOT_BIN:?Set DEVICE_BOT_BIN to a Windows build with desktop recording}"
    "$DEVICE_BOT_BIN" --version
    "$DEVICE_BOT_BIN" --check-install
    return 0
    ;;
esac

export DEVICE_BOT_INSTALL_DIR="${DEVICE_BOT_INSTALL_DIR:-$HOME/.local/bin}"
export PATH="$DEVICE_BOT_INSTALL_DIR:$PATH"

device_bot_ready() {
  local actual
  command -v "$DEVICE_BOT_BIN" >/dev/null 2>&1 || return 1
  actual="$("$DEVICE_BOT_BIN" --version)" || return 1
  if [[ -n "${DEVICE_BOT_VERSION:-}" && "$actual" != "${DEVICE_BOT_VERSION#v}" ]]; then
    return 1
  fi
  "$DEVICE_BOT_BIN" --check-install
}

if [[ -n "${DEVICE_BOT_BIN:-}" ]]; then
  # An explicit runner override is owned by its operator; never replace it.
  if ! device_bot_ready; then
    echo "Configured DEVICE_BOT_BIN is missing, invalid, or has the wrong version: $DEVICE_BOT_BIN" >&2
    exit 1
  fi
else
  export DEVICE_BOT_BIN=device-bot
  if ! device_bot_ready; then
    echo "Installing device-bot and its versioned assets..."
    device_bot_installer="$(mktemp)"
    if ! curl --fail --silent --show-error --location --retry 3 \
      --proto '=https' --tlsv1.2 \
      https://raw.githubusercontent.com/ho-doan/device-bot/b94571c090647acc5c57c9af4323b933f0866f23/scripts/install.sh \
      --output "$device_bot_installer"; then
      rm -f "$device_bot_installer"
      exit 1
    fi
    if ! sh "$device_bot_installer"; then
      rm -f "$device_bot_installer"
      echo "device-bot installation failed. Check that the requested GitHub release and checksums.txt are published." >&2
      exit 1
    fi
    rm -f "$device_bot_installer"
    unset device_bot_installer
    # Use exactly the installed binary, even if another copy was found earlier.
    export DEVICE_BOT_BIN="$DEVICE_BOT_INSTALL_DIR/device-bot"
    device_bot_ready
  fi
fi
"$DEVICE_BOT_BIN" --version

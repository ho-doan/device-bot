#!/usr/bin/env bash
# Install the pinned go-ios CLI locally. The binary is intentionally ignored
# by git so a checkout stays small and repeatable.
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
go_binary="${GO_BINARY:-go}"
tool_bin="$repository_root/tools/go/bin"
version="${DEVICE_KIT_GO_IOS_VERSION:-v1.3.2}"

if ! command -v "$go_binary" >/dev/null 2>&1; then
  echo "Go is required. Install Go, then rerun $0." >&2
  exit 1
fi

mkdir -p "$tool_bin"
echo "Installing go-ios $version in $tool_bin" >&2
GOBIN="$tool_bin" "$go_binary" install "github.com/danielpaulus/go-ios@$version"
echo "Installed $tool_bin/go-ios" >&2

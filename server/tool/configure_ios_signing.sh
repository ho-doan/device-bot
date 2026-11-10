#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$script_dir/../crates/robot-ios-real/xctest-runner/configure_ios_signing.sh" "$@"

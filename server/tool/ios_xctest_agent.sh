#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: MOBILE_MCP_IOS_DEVICE=<xcode-udid> $0 build|start" >&2
  echo "  build  compile and sign the real-device XCTest agent" >&2
  echo "  start  run the already-built agent and connect it to Device Bot" >&2
  exit 2
}

command="${1:-}"
[[ "$command" == "build" || "$command" == "start" ]] || usage

device="${MOBILE_MCP_IOS_DEVICE:-${DEVICE_KIT_DEVICE:-}}"
[[ -n "$device" ]] || usage

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
runner_dir="$(cd "$script_dir/../crates/robot-ios-real/xctest-runner" && pwd)"
project="$runner_dir/MobileMcpRealRunner.xcodeproj"
derived_data="$runner_dir/build"
destination="platform=iOS,id=$device"
server_dir="$(cd "$script_dir/.." && pwd)"

common=(
  -project "$project"
  -scheme MobileMcpRealRunner
  -destination "$destination"
  -derivedDataPath "$derived_data"
)
case "$command" in
  build)
    echo "Provisioning and building Device Bot real-device XCTest agent -> $destination" >&2
    cargo run --quiet --manifest-path "$server_dir/Cargo.toml" \
      -p mobile-mcp-server -- --provision-ios-xctest "$device"
    ;;
  start)
    hosts="${MOBILE_MCP_IOS_AGENT_HOSTS:-}"
    if [[ -z "$hosts" ]]; then
      for interface in en0 en1; do
        hosts="$(ipconfig getifaddr "$interface" 2>/dev/null || true)"
        [[ -n "$hosts" ]] && break
      done
    fi
    [[ -n "$hosts" ]] || {
      echo "Set MOBILE_MCP_IOS_AGENT_HOSTS to this Mac's LAN IPv4 address." >&2
      exit 2
    }
    port="${MOBILE_MCP_IOS_AGENT_PORT:-18787}"
    token="${MOBILE_MCP_IOS_AGENT_TOKEN:-}"
    [[ -n "$token" ]] || {
      echo "Set MOBILE_MCP_IOS_AGENT_TOKEN to the token used by Device Bot." >&2
      exit 2
    }
    export TEST_RUNNER_MOBILE_MCP_HOSTS="$hosts"
    export TEST_RUNNER_MOBILE_MCP_PORT="$port"
    export TEST_RUNNER_MOBILE_MCP_TOKEN="$token"
    echo "Starting Device Bot XCTest agent -> $hosts:$port" >&2
    xcodebuild test-without-building "${common[@]}" \
      -only-testing:MobileMcpRealRunnerUITests/MobileMcpRealRunnerUITests/testServe
    ;;
esac

#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: DEVICE_BOT_IOS_DEVICE=<xcode-udid> $0 build|start" >&2
  echo "  build  compile and sign the real-device XCTest agent" >&2
  echo "  start  run the already-built agent and connect it to Device Bot" >&2
  exit 2
}

command="${1:-}"
[[ "$command" == "build" || "$command" == "start" ]] || usage

device="${DEVICE_BOT_IOS_DEVICE:-${DEVICE_KIT_DEVICE:-}}"
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
      -p device-bot -- --provision-ios-xctest "$device"
    ;;
  start)
    hosts="${DEVICE_BOT_IOS_AGENT_HOSTS:-}"
    if [[ -z "$hosts" ]]; then
      while IFS= read -r interface; do
        case "$interface" in
          en*) ;;
          *) continue ;;
        esac
        address="$(ipconfig getifaddr "$interface" 2>/dev/null || true)"
        [[ -n "$address" && "$address" != 127.* && "$address" != 169.254.* ]] || continue
        hosts="${hosts:+$hosts,}$address"
      done < <(ifconfig -l | tr ' ' '\n')
    fi
    [[ -n "$hosts" ]] || {
      echo "Set DEVICE_BOT_IOS_AGENT_HOSTS to this Mac's Wi-Fi or Ethernet address." >&2
      exit 2
    }
    port="${DEVICE_BOT_IOS_AGENT_PORT:-18787}"
    token="${DEVICE_BOT_IOS_AGENT_TOKEN:-}"
    [[ -n "$token" ]] || {
      echo "Set DEVICE_BOT_IOS_AGENT_TOKEN to the token used by Device Bot." >&2
      exit 2
    }
    # Xcode strips the TEST_RUNNER_ prefix before the on-device test reads
    # DEVICE_BOT_HOSTS, DEVICE_BOT_PORT, and DEVICE_BOT_TOKEN.
    export TEST_RUNNER_DEVICE_BOT_DEVICE_ID="$device"
    export TEST_RUNNER_DEVICE_BOT_HOSTS="$hosts"
    export TEST_RUNNER_DEVICE_BOT_PORT="$port"
    export TEST_RUNNER_DEVICE_BOT_TOKEN="$token"
    echo "Starting Device Bot XCTest agent -> $hosts:$port" >&2
    xcodebuild test-without-building "${common[@]}" \
      -only-testing:MobileMcpRealRunnerUITests/MobileMcpRealRunnerUITests/testServe
    ;;
esac

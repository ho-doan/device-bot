#!/usr/bin/env bash
# Start or inspect the local go-ios agent used by the real-device MCP driver.
set -euo pipefail

usage() {
  echo "Usage: DEVICE_KIT_DEVICE=<iPhone-UDID> $0 start|status" >&2
  exit 2
}

command="${1:-}"
[[ "$command" == "start" || "$command" == "status" ]] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ios_binary="${GO_IOS_PATH:-$repository_root/tools/go/bin/go-ios}"
udid="${DEVICE_KIT_DEVICE:-}"
agent_port="${GO_IOS_AGENT_PORT:-28100}"
pair_records="${DEVICE_KIT_GO_IOS_PAIR_RECORDS:-/Users/Shared/device-kit-go-ios}"

[[ -n "$udid" ]] || usage
if [[ ! -x "$ios_binary" ]]; then
  echo "go-ios is missing. Run tool/install_go_ios.sh first." >&2
  exit 1
fi

case "$command" in
  status)
    GO_IOS_AGENT_PORT="$agent_port" "$ios_binary" \
      --tunnel-info-port="$agent_port" tunnel ls
    ;;
  start)
    echo "Starting go-ios userspace tunnel on port $agent_port for $udid" >&2
    echo "This needs macOS administrator approval to coordinate with remoted." >&2
    exec sudo env \
      GO_IOS_AGENT_PORT="$agent_port" \
      "$ios_binary" \
      --udid="$udid" \
      --tunnel-info-port="$agent_port" \
      tunnel start --userspace --pair-record-path="$pair_records"
    ;;
esac

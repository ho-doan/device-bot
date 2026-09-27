#!/usr/bin/env bash
# Build the XCTest host agent once, then start it without compiling again.
#
#   DEVICE_KIT_IOS_PROJECT=/path/to/ios/Runner.xcodeproj \
#     tool/ios_xctest_agent.sh build
#
#   DEVICE_KIT_IOS_PROJECT=/path/to/ios/Runner.xcodeproj \
#     tool/ios_xctest_agent.sh start
set -euo pipefail

usage() {
  echo "Usage: DEVICE_KIT_IOS_PROJECT=<Runner.xcodeproj> $0 build|start" >&2
  echo "  build  compile + sign once (build-for-testing)" >&2
  echo "  start  launch the last products (test-without-building)" >&2
  exit 2
}

command="${1:-}"
[[ "$command" == "build" || "$command" == "start" ]] || usage

project="${DEVICE_KIT_IOS_PROJECT:-}"
if [[ -z "$project" ]]; then
  echo "Set DEVICE_KIT_IOS_PROJECT to the UI-test .xcodeproj of the installed runner." >&2
  exit 1
fi

scheme="${DEVICE_KIT_IOS_SCHEME:-Runner}"
test="${DEVICE_KIT_IOS_TEST:-RunnerUITests/DeviceKitHttpAgentTests/testServe}"
port="${DEVICE_KIT_AGENT_PORT:-18787}"
deployment_target="${DEVICE_KIT_IOS_DEPLOYMENT_TARGET:-15.0}"
allow_provisioning_updates="${DEVICE_KIT_ALLOW_PROVISIONING_UPDATES:-0}"
development_team="${DEVICE_KIT_IOS_TEAM:-}"

if [[ -n "${DEVICE_KIT_IOS_DESTINATION:-}" ]]; then
  destination="$DEVICE_KIT_IOS_DESTINATION"
elif [[ -n "${DEVICE_KIT_DEVICE:-}" ]]; then
  # `xcodebuild` exposes physical-device UDIDs as destination ids. Preserve
  # friendly names for users who prefer `DEVICE_KIT_DEVICE=My iPhone`.
  if xcodebuild -project "$project" -scheme "$scheme" -showdestinations 2>/dev/null \
      | grep -Fq "id:$DEVICE_KIT_DEVICE,"; then
    destination="platform=iOS,id=$DEVICE_KIT_DEVICE"
  else
    destination="platform=iOS,name=$DEVICE_KIT_DEVICE"
  fi
else
  destination="generic/platform=iOS"
  if dest_line="$(xcodebuild -project "$project" -scheme "$scheme" -showdestinations 2>/dev/null \
      | awk '/platform:iOS,/{print; exit}')"; then
    dest_id="$(printf '%s\n' "$dest_line" | sed -n 's/.*id:\([^,}]*\).*/\1/p' | tr -d ' ')"
    dest_name="$(printf '%s\n' "$dest_line" | sed -n 's/.*name:\([^,}]*\).*/\1/p' | sed 's/^ *//')"
    if [[ -n "$dest_id" ]]; then
      destination="platform=iOS,id=$dest_id"
    elif [[ -n "$dest_name" ]]; then
      destination="platform=iOS,name=$dest_name"
    fi
  fi
fi

hosts="${DEVICE_KIT_HOSTS:-}"
if [[ -z "$hosts" ]]; then
  hosts="$(ifconfig -a 2>/dev/null \
    | awk '/inet / && $2 != "127.0.0.1" {print $2}' \
    | paste -sd, -)"
fi
if [[ -z "$hosts" ]]; then
  echo "No non-loopback IPv4 address found for DEVICE_KIT_HOSTS." >&2
  exit 1
fi

export TEST_RUNNER_DEVICE_KIT_HOSTS="$hosts"
export TEST_RUNNER_DEVICE_KIT_PORT="$port"
export DEVICE_KIT_HOSTS="$hosts"
export DEVICE_KIT_PORT="$port"
if command -v flutter >/dev/null 2>&1; then
  export FLUTTER_ROOT="${FLUTTER_ROOT:-$(dirname "$(dirname "$(command -v flutter)")")}"
fi

common=(
  -project "$project"
  -scheme "$scheme"
  -destination "$destination"
  IPHONEOS_DEPLOYMENT_TARGET="$deployment_target"
)

# Provisioning modifies state in the Apple Developer portal, so it is opt-in.
# Use this only after the intended Apple Developer account/team is selected in
# Xcode and authenticated on this Mac.
if [[ "$allow_provisioning_updates" == "1" ]]; then
  common+=( -allowProvisioningUpdates )
fi
if [[ -n "$development_team" ]]; then
  common+=( DEVELOPMENT_TEAM="$development_team" )
fi

case "$command" in
  build)
    echo "Building XCTest agent once -> $destination" >&2
    xcodebuild build-for-testing "${common[@]}"
    ;;
  start)
    echo "Starting last XCTest agent (no compile) -> $destination" >&2
    echo "Hosts $hosts  port $port" >&2
    xcodebuild test-without-building \
      "${common[@]}" \
      -only-testing:"$test"
    ;;
esac

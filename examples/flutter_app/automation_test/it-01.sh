#!/usr/bin/env bash
# IT-01 via the installed device-bot MCP server (stdio), not robot-flow.
# DEVICE_ID=<adb-serial> bash examples/flutter_app/automation_test/it-01.sh
# PLATFORM=ios-sim DEVICE_ID=<udid> bash examples/flutter_app/automation_test/it-01.sh
# Optional: APP_ID, DEVICE_BOT_BIN, APP_PATH (or APK_PATH), RPC_TIMEOUT.
# Requires an unlocked Android device or booted iOS Simulator and the Orbit app.
# YAML remains a reference scenario pending implementation of an MCP YAML client.
# GoRouter URIs and takeException() remain assertions in the Flutter test.
set -euo pipefail
: "${DEVICE_ID:?Set DEVICE_ID to the Android serial or iOS Simulator UDID}"
readonly PLATFORM="${PLATFORM:-android}"
case "$PLATFORM" in
  android) APP_ID="${APP_ID:-com.example.flutter_app}"; PLATFORM_COMMAND="${ANDROID_ADB_PATH:-adb}" ;;
  ios-sim) APP_ID="${APP_ID:-com.example.flutterApp}"; PLATFORM_COMMAND="${XCRUN_PATH:-xcrun}"; unset SDKROOT ;;
  *) printf 'PLATFORM must be android or ios-sim\n' >&2; exit 2 ;;
esac
readonly APP_ID PLATFORM_COMMAND
readonly RPC_TIMEOUT="${RPC_TIMEOUT:-180}"
[[ "$RPC_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { printf 'Invalid RPC_TIMEOUT\n' >&2; exit 2; }
for dependency in jq "${DEVICE_BOT_BIN:-device-bot}" "$PLATFORM_COMMAND"; do
  command -v "$dependency" >/dev/null || { printf 'Missing: %s\n' "$dependency" >&2; exit 2; }
done
readonly ARTIFACT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-bot-it01.XXXXXX")"
printf 'Artifacts: %s\n' "$ARTIFACT_DIR"
mkfifo "$ARTIFACT_DIR/input" "$ARTIFACT_DIR/output"
"${DEVICE_BOT_BIN:-device-bot}" <"$ARTIFACT_DIR/input" >"$ARTIFACT_DIR/output" &
readonly MCP_PID=$!
exec 3>"$ARTIFACT_DIR/input"
exec 4<"$ARTIFACT_DIR/output"
next_id=2
last_response=''
initialized=false

receive() {
  local id="$1" line deadline remaining
  deadline=$((SECONDS + RPC_TIMEOUT))
  while ((SECONDS < deadline)); do
    remaining=$((deadline - SECONDS))
    if ! IFS= read -r -t "$remaining" line <&4; then
      printf 'MCP timeout/EOF for request %s\n' "$id" >&2; return 1
    fi
    [[ "$(jq -r '.id // empty' <<<"$line")" == "$id" ]] || continue
    last_response="$line"
    printf '%s\n' "$line" >>"$ARTIFACT_DIR/responses.jsonl"
    if jq -e 'has("error") or .result.isError == true' <<<"$line" >/dev/null; then
      jq '{error,content:.result.content}' <<<"$line" >&2; return 1
    fi
    return 0
  done
  printf 'MCP timeout for request %s\n' "$id" >&2; return 1
}

call_tool() {
  local name="$1" arguments="$2" id="$next_id"
  next_id=$((next_id + 1))
  jq -cn --argjson id "$id" --arg name "$name" --argjson args "$arguments" \
    '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:$name,arguments:$args}}' >&3
  receive "$id"
}
readonly DEVICE_ARGS="$(jq -cn --arg d "$DEVICE_ID" '{deviceId:$d}')"
screenshot() {
  call_tool mobile_take_screenshot "$DEVICE_ARGS" || return 1
  jq -er '.result.content[] | select(.type=="image" and .mimeType=="image/png") | .data' \
    <<<"$last_response" | base64 --decode >"$ARTIFACT_DIR/$1.png"
}
cleanup() {
  local status=$?
  trap - EXIT
  if ((status != 0)) && [[ "$initialized" == true ]]; then screenshot failure || true; fi
  exec 3>&- 4<&-
  kill "$MCP_PID" 2>/dev/null || true
  wait "$MCP_PID" 2>/dev/null || true
  unlink "$ARTIFACT_DIR/input"
  unlink "$ARTIFACT_DIR/output"
  exit "$status"
}
trap cleanup EXIT

scroll_down() {
  local dimensions
  if [[ "$PLATFORM" == ios-sim ]]; then
    # XCTest bounds/input use points, whereas simulator screenshots use pixels.
    # Derive a safe gesture region from hittable elements in the native tree.
    call_tool mobile_list_elements_on_screen "$(jq -cn --arg d "$DEVICE_ID" \
      '{deviceId:$d,maxResults:100,timeoutMs:60000}')"
    dimensions="$(jq -ce '[.result.structuredContent.elements[] |
      select(.clickable and .bounds.left >= 0 and .bounds.top >= 0) | .bounds] |
      {w:(map(.right)|max),h:(map(.bottom)|max)} | select(.w>0 and .h>0)' <<<"$last_response")"
  else
  screenshot scroll
  dimensions="$(od -An -v -t u1 -j 16 -N 8 "$ARTIFACT_DIR/scroll.png" | jq -s '
    {w:(.[0]*16777216+.[1]*65536+.[2]*256+.[3]),h:(.[4]*16777216+.[5]*65536+.[6]*256+.[7])}')"
  fi
  call_tool mobile_swipe_on_screen "$(jq -cn --arg d "$DEVICE_ID" --argjson s "$dimensions" \
    '{deviceId:$d,startX:($s.w*0.5),startY:($s.h*0.75),endX:($s.w*0.5),endY:($s.h*0.4),durationMs:350}')"
}
find_element() {
  local label="$1" tap="${2:-false}" scroll="${3:-false}" attempt matches count
  for ((attempt=0; attempt<8; attempt++)); do
    call_tool mobile_list_elements_on_screen "$(jq -cn --arg d "$DEVICE_ID" --arg f "$label" \
      '{deviceId:$d,filter:$f,maxResults:100,timeoutMs:60000}')"
    jq '.result.structuredContent.elements' <<<"$last_response" >"$ARTIFACT_DIR/last-query.json"
    matches="$(jq -c --arg label "$label" --argjson tap "$tap" '
      [.[] | select(([.text // "",.contentDescription // ""] |
        any(. == $label or (split("\n") | index($label) != null)))
        and .bounds.left >= 0 and .bounds.top >= 0
        and .bounds.right > .bounds.left and .bounds.bottom > .bounds.top
        and (($tap | not) or (.enabled and .clickable)))]' "$ARTIFACT_DIR/last-query.json")"
    count="$(jq length <<<"$matches")"
    if ((count > 0)); then
      if [[ "$tap" == true ]]; then
        [[ "$count" == 1 ]] || { printf 'Ambiguous: %s\n' "$label" >&2; return 1; }
        call_tool mobile_click_on_screen_at_coordinates "$(jq -c --arg d "$DEVICE_ID" '
          .[0].bounds | {deviceId:$d,x:((.left+.right)/2),y:((.top+.bottom)/2)}' <<<"$matches")"
      fi
      printf 'PASS: %s\n' "$label"; return 0
    fi
    if [[ "$scroll" == true ]] && ((attempt >= 2 && attempt < 7)); then scroll_down; fi
    sleep 0.5
  done
  printf 'FAIL: Missing %s (see %s)\n' "$label" "$ARTIFACT_DIR" >&2; return 1
}

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"orbit-it01","version":"1.0"}}}' >&3
receive 1
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&3
initialized=true
# The server has no terminate tool; only cold-stop uses the platform CLI.
if [[ -n "${APP_PATH:-${APK_PATH:-}}" ]]; then
  call_tool mobile_install_app "$(jq -cn --arg d "$DEVICE_ID" --arg p "${APP_PATH:-$APK_PATH}" '{deviceId:$d,apkPath:$p}')"
fi
if [[ "$PLATFORM" == android ]]; then
  "$PLATFORM_COMMAND" -s "$DEVICE_ID" shell am force-stop "$APP_ID"
else
  if ! "$PLATFORM_COMMAND" simctl terminate "$DEVICE_ID" "$APP_ID" 2>"$ARTIFACT_DIR/terminate.log"; then
    if ! grep -qi 'found nothing to terminate' "$ARTIFACT_DIR/terminate.log"; then
      cat "$ARTIFACT_DIR/terminate.log" >&2; exit 1
    fi
  fi
fi
call_tool mobile_launch_app "$(jq -cn --arg d "$DEVICE_ID" --arg a "$APP_ID" '{deviceId:$d,appId:$a}')"
find_element 'Welcome back.'
screenshot login
find_element 'Try demo workspace' true true
find_element 'A fresh start, Alex.'
find_element '1/3' false true
screenshot workspace
printf 'PASS: IT-01 %s UI via MCP. Router URI/Flutter exceptions are not checked.\n' "$PLATFORM"

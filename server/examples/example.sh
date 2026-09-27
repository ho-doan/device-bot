#!/usr/bin/env bash
# Kịch bản MCP: mở Chrome, tab mới, tìm babyshark, mở video đầu tiên, phát và quay màn hình.
# Cần jq để đọc structuredContent trả về từ MCP.
#
# Chạy: ./server/examples/example.sh

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SERVER_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
readonly DEVICE_ID="${DEVICE_ID:-macos-host}"
readonly APP_ID="${APP_ID:-Google Chrome}"
readonly SETTLE_SECONDS="${SETTLE_SECONDS:-3}"

command -v jq >/dev/null || { printf 'Kịch bản cần jq.\n' >&2; exit 1; }
[[ "$SETTLE_SECONDS" =~ ^[0-9]+$ ]] || {
  printf 'SETTLE_SECONDS phải là số nguyên không âm.\n' >&2
  exit 2
}

readonly SESSION_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-robot-mcp.XXXXXX")"
readonly INPUT_PIPE="$SESSION_DIR/input"
readonly OUTPUT_PIPE="$SESSION_DIR/output"
mkfifo "$INPUT_PIPE" "$OUTPUT_PIPE"
cargo run -q --manifest-path "$SERVER_DIR/Cargo.toml" -p mobile-mcp-server \
  <"$INPUT_PIPE" >"$OUTPUT_PIPE" &
readonly MCP_SERVER_PID=$!
exec 3>"$INPUT_PIPE"
exec 4<"$OUTPUT_PIPE"

next_id=1
recording_started=false
last_response=''

send() {
  printf '%s\n' "$1" >&3
}

receive() {
  local expected_id="$1" response
  while IFS= read -r response <&4; do
    if [[ "$(jq -r '.id // empty' <<<"$response")" == "$expected_id" ]]; then
      last_response="$response"
      return 0
    fi
  done
  printf 'MCP server đã đóng trước khi trả response id=%s.\n' "$expected_id" >&2
  return 1
}

call_tool() {
  local tool_name="$1" arguments="$2" request_id="$next_id"
  next_id=$((next_id + 1))
  send "$(jq -cn --argjson id "$request_id" --arg name "$tool_name" --argjson arguments "$arguments" \
    '{jsonrpc:"2.0", id:$id, method:"tools/call", params:{name:$name, arguments:$arguments}}')"
  receive "$request_id"
  if [[ "$(jq -r '.result.isError // false' <<<"$last_response")" == true ]]; then
    jq -r '.result.content[]?.text // "MCP tool thất bại"' <<<"$last_response" >&2
    return 1
  fi
}

tap_first_element() {
  local filter="$1" clickable_only="$2" arguments candidate x y
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    '{deviceId:$device, filter:$filter, maxResults:50}')"
  call_tool mobile_list_elements_on_screen "$arguments"
  candidate="$(jq -cer --argjson clickable "$clickable_only" '
    .result.structuredContent.elements
    | map(select(.bounds != null and ($clickable | not or .clickable)))
    | first
    | {x: ((.bounds.left + .bounds.right) / 2), y: ((.bounds.top + .bounds.bottom) / 2)}
  ' <<<"$last_response")" || {
    printf 'Không tìm thấy element %q trên %s.\n' "$filter" "$DEVICE_ID" >&2
    return 1
  }
  x="$(jq -r '.x | round' <<<"$candidate")"
  y="$(jq -r '.y | round' <<<"$candidate")"
  call_tool mobile_click_on_screen_at_coordinates \
    "$(jq -cn --arg device "$DEVICE_ID" --argjson x "$x" --argjson y "$y" '{deviceId:$device, x:$x, y:$y}')"
}

stop_recording() {
  if [[ "$recording_started" == true ]]; then
    call_tool mobile_stop_screen_recording "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')" || true
    recording_started=false
    jq -r '.result.content[]?.text // empty' <<<"$last_response"
  fi
}

cleanup() {
  local status=$?
  stop_recording
  kill "$MCP_SERVER_PID" 2>/dev/null || true
  exec 3>&- 4<&-
  wait "$MCP_SERVER_PID" 2>/dev/null || true
  rm -f "$INPUT_PIPE" "$OUTPUT_PIPE"
  rmdir "$SESSION_DIR" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT

# MCP lifecycle.
send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"device-robot-example","version":"1.0"}}}'
receive 1
send '{"jsonrpc":"2.0","method":"notifications/initialized"}'
next_id=2

call_tool mobile_list_available_devices '{}'
call_tool mobile_start_screen_recording "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')"
recording_started=true
call_tool mobile_launch_app "$(jq -cn --arg device "$DEVICE_ID" --arg app "$APP_ID" '{deviceId:$device, appId:$app}')"
sleep "$SETTLE_SECONDS"

# UI native của platform: Chrome tab strip và ô địa chỉ/tìm kiếm.
tap_first_element 'New Tab' true
tap_first_element 'Address and search bar' false
call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, text:"babyshark\n"}')"
sleep "$SETTLE_SECONDS"

# UI web: kết quả video đầu tiên, sau đó nút Play của video.
tap_first_element 'Baby Shark' true
sleep "$SETTLE_SECONDS"
tap_first_element 'Play' true
sleep "$SETTLE_SECONDS"

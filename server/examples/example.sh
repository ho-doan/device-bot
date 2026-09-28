#!/usr/bin/env bash
# Kịch bản MCP: mở Chrome Incognito, tìm babyshark, mở video đầu tiên, phát và quay màn hình.
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
readonly CHROME_PROFILE_DIR="$SESSION_DIR/chrome-profile"
# A private profile forces a new Chrome process, so renderer accessibility is
# enabled even when the user's ordinary Chrome profile is already running.
export MOBILE_MCP_MACOS_APP_ARGS="${MOBILE_MCP_MACOS_APP_ARGS:---user-data-dir=$CHROME_PROFILE_DIR --incognito --no-first-run --no-default-browser-check --force-renderer-accessibility}"
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
  local filter="$1" clickable_only="$2" arguments candidate x y attempt max_results
  max_results=1
  [[ "$clickable_only" == true ]] && max_results=10
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    --argjson max_results "$max_results" \
    '{deviceId:$device, filter:$filter, maxResults:$max_results}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    if [[ "${DEVICE_ROBOT_DEBUG:-}" == "1" ]]; then
      local debug_line
      debug_line="$(jq -c --arg filter "$filter" '.result.structuredContent.elements | {filter:$filter, elements:.}' \
        <<<"$last_response")"
      printf '%s\n' "$debug_line" >&2
      if [[ -n "${DEVICE_ROBOT_DEBUG_LOG:-}" ]]; then
        printf '%s\n' "$debug_line" >>"$DEVICE_ROBOT_DEBUG_LOG"
      fi
    fi
    candidate="$(jq -cer --argjson clickable "$clickable_only" '
      [
        .result.structuredContent.elements[]
        | select(
            .bounds.left != null and .bounds.right != null and
            .bounds.top != null and .bounds.bottom != null and
            (($clickable == false) or (.clickable == true))
          )
        | {x: ((.bounds.left + .bounds.right) / 2), y: ((.bounds.top + .bounds.bottom) / 2)}
      ]
      | first?
      ' <<<"$last_response" 2>/dev/null || true)"
    if [[ -n "$candidate" && "$candidate" != null ]]; then
      x="$(jq -r '.x | round' <<<"$candidate")"
      y="$(jq -r '.y | round' <<<"$candidate")"
      call_tool mobile_click_on_screen_at_coordinates \
        "$(jq -cn --arg device "$DEVICE_ID" --argjson x "$x" --argjson y "$y" '{deviceId:$device, x:$x, y:$y}')"
      return
    fi
    sleep 0.25
  done
  printf 'Không tìm thấy element %q trên %s sau tối đa 3 giây.\n' "$filter" "$DEVICE_ID" >&2
  return 1
}

tap_first_video_result() {
  local arguments candidate x y attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" \
    '{deviceId:$device, filter:"Baby Shark", maxResults:20}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    candidate="$(jq -cer '
      [
        .result.structuredContent.elements[]
        | select(
            .className == "AXLink" and .clickable == true and
            (.contentDescription // "" | contains("YouTube")) and
            .bounds.left >= 200 and .bounds.right <= 750 and
            .bounds.top != null and .bounds.bottom != null
          )
        | {x: ((.bounds.left + .bounds.right) / 2), y: ((.bounds.top + .bounds.bottom) / 2)}
      ] | first?
    ' <<<"$last_response" 2>/dev/null || true)"
    if [[ -n "$candidate" && "$candidate" != null ]]; then
      x="$(jq -r '.x | round' <<<"$candidate")"
      y="$(jq -r '.y | round' <<<"$candidate")"
      call_tool mobile_click_on_screen_at_coordinates \
        "$(jq -cn --arg device "$DEVICE_ID" --argjson x "$x" --argjson y "$y" '{deviceId:$device, x:$x, y:$y}')"
      return
    fi
    sleep 0.25
  done
  printf 'Không tìm thấy kết quả video YouTube đầu tiên trên %s sau tối đa 3 giây.\n' "$DEVICE_ID" >&2
  return 1
}

require_clickable_element() {
  local filter="$1" arguments candidate attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    '{deviceId:$device, filter:$filter, maxResults:10}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    candidate="$(jq -cer '
      .result.structuredContent.elements[]
      | select(.clickable == true and .bounds.left != null and .bounds.right != null)
    ' <<<"$last_response" 2>/dev/null || true)"
    [[ -n "$candidate" ]] && return 0
    sleep 0.25
  done
  printf 'Không xác nhận được element có thể click %q trên %s sau tối đa 3 giây.\n' "$filter" "$DEVICE_ID" >&2
  return 1
}

dismiss_translate_popup_if_present() {
  local popup close candidate x y
  call_tool mobile_list_elements_on_screen \
    "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, filter:"Translate this page?", maxResults:10}')"
  popup="$(jq -cer '.result.structuredContent.elements[]?.bounds | select(.left != null and .right != null and .top != null and .bottom != null)' <<<"$last_response" 2>/dev/null || true)"
  [[ -n "$popup" ]] || return 0

  call_tool mobile_list_elements_on_screen \
    "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, filter:"Close", maxResults:20}')"
  candidate="$(jq -cer --argjson popup "$popup" '
    [
      .result.structuredContent.elements[]
      | select(.clickable == true and .bounds.left != null and .bounds.right != null and .bounds.top != null and .bounds.bottom != null)
      | {x: ((.bounds.left + .bounds.right) / 2), y: ((.bounds.top + .bounds.bottom) / 2)}
      | select(.x >= $popup.left and .x <= $popup.right and .y >= $popup.top and .y <= $popup.bottom)
    ] | first?
  ' <<<"$last_response" 2>/dev/null || true)"
  if [[ -n "$candidate" && "$candidate" != null ]]; then
    x="$(jq -r '.x | round' <<<"$candidate")"
    y="$(jq -r '.y | round' <<<"$candidate")"
    call_tool mobile_click_on_screen_at_coordinates \
      "$(jq -cn --arg device "$DEVICE_ID" --argjson x "$x" --argjson y "$y" '{deviceId:$device, x:$x, y:$y}')"
  fi
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

# UI native của platform: ưu tiên mở tab mới, nhưng tiếp tục ở tab hiện tại
# nếu Chrome chưa expose tab strip qua accessibility trong thời gian khởi động.
tap_first_element 'New Tab' true || printf 'Không tìm thấy New Tab; dùng tab hiện tại.\n' >&2
tap_first_element 'Address and search bar' false
call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, text:"babyshark\n"}')"
sleep "$SETTLE_SECONDS"
dismiss_translate_popup_if_present

# UI web: chọn chính xác link của kết quả YouTube đầu tiên, không nhầm nút
# gợi ý truy vấn "baby shark" nằm trong cùng kết quả filter.
tap_first_video_result
sleep "$SETTLE_SECONDS"
tap_first_element 'Play' true
# Khi video đang chạy, accessibility của YouTube đổi action của player từ
# Play sang Pause. Đây là assertion cho step cuối, không chỉ là click thành công.
require_clickable_element 'Pause'
sleep "$SETTLE_SECONDS"

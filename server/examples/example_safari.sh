#!/usr/bin/env bash
# Kịch bản MCP Safari: tìm ChatGPT, nhập "hello", gửi và quay màn hình.
# Dùng khi thiết bị không có Chrome. Cần jq để đọc structuredContent trả về từ MCP.
#
# Chạy macOS: ./server/examples/example_safari.sh
# Chạy iOS Simulator: DEVICE_ID=<simulator-id> APP_ID=com.apple.mobilesafari ./server/examples/example_safari.sh

set -euo pipefail

# An inherited SDKROOT may point at an SDK removed by an Xcode update. Let
# xcrun/xcodebuild select the active Xcode SDK for this standalone scenario.
unset SDKROOT

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SERVER_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
readonly DEVICE_ID="${DEVICE_ID:-macos-host}"
# `Safari` is the macOS application name. Override APP_ID with the platform's
# Safari identifier (for example, com.apple.mobilesafari on an iOS Simulator).
readonly APP_ID="${APP_ID:-Safari}"
readonly SETTLE_SECONDS="${SETTLE_SECONDS:-3}"
# XCTest needs a larger request budget on iOS to attach to Safari. This is not
# a build budget: after warm-up, requests still use test-without-building.
if [[ "$APP_ID" == "com.apple.mobilesafari" ]]; then
  readonly LIST_ELEMENTS_TIMEOUT_MS="${LIST_ELEMENTS_TIMEOUT_MS:-60000}"
else
  readonly LIST_ELEMENTS_TIMEOUT_MS="${LIST_ELEMENTS_TIMEOUT_MS:-15000}"
fi
readonly XCTEST_WARMUP_TIMEOUT_MS="${XCTEST_WARMUP_TIMEOUT_MS:-60000}"
command -v jq >/dev/null || { printf 'Kịch bản cần jq.\n' >&2; exit 1; }
[[ "$SETTLE_SECONDS" =~ ^[0-9]+$ ]] || {
  printf 'SETTLE_SECONDS phải là số nguyên không âm.\n' >&2
  exit 2
}
[[ "$LIST_ELEMENTS_TIMEOUT_MS" =~ ^[1-9][0-9]*$ && "$LIST_ELEMENTS_TIMEOUT_MS" -le 60000 ]] || {
  printf 'LIST_ELEMENTS_TIMEOUT_MS phải là số nguyên trong khoảng 1..60000.\n' >&2
  exit 2
}
[[ "$XCTEST_WARMUP_TIMEOUT_MS" =~ ^[1-9][0-9]*$ && "$XCTEST_WARMUP_TIMEOUT_MS" -le 60000 ]] || {
  printf 'XCTEST_WARMUP_TIMEOUT_MS phải là số nguyên trong khoảng 1..60000.\n' >&2
  exit 2
}

readonly SESSION_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-robot-mcp.XXXXXX")"
readonly INPUT_PIPE="$SESSION_DIR/input"
readonly OUTPUT_PIPE="$SESSION_DIR/output"
# Keep the XCTest products in the workspace target directory so separate MCP
# processes and later example/test runs share one machine-local build cache.
readonly XCTEST_DERIVED_DATA_DIR="${MOBILE_MCP_IOS_XCTEST_DERIVED_DATA:-$SERVER_DIR/target/mobile-mcp-ios-xctest-derived-data-persistent-agent}"
export MOBILE_MCP_IOS_XCTEST_DERIVED_DATA="$XCTEST_DERIVED_DATA_DIR"
# The server always starts an iOS XCTest bridge, although this Safari example
# uses an iOS Simulator and does not need it. An ephemeral port keeps repeated
# example runs from colliding with another local server.
export MOBILE_MCP_IOS_AGENT_BIND="${MOBILE_MCP_IOS_AGENT_BIND:-127.0.0.1:0}"
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
  if [[ "$tool_name" == mobile_list_elements_on_screen ]]; then
    arguments="$(jq -c --argjson timeout "$LIST_ELEMENTS_TIMEOUT_MS" '.timeoutMs //= $timeout' <<<"$arguments")"
  fi
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
  local filter="$1" clickable_only="$2" max_attempts="${3:-12}" tap_anchor="${4:-center}" arguments candidate x y attempt max_results
  max_results=1
  [[ "$clickable_only" == true ]] && max_results=10
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    --argjson max_results "$max_results" \
    '{deviceId:$device, filter:$filter, maxResults:$max_results}')"
  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
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
    candidate="$(jq -cer --argjson clickable "$clickable_only" --arg anchor "$tap_anchor" '
      [
        .result.structuredContent.elements[]
        | select(
            .bounds.left != null and .bounds.right != null and
            .bounds.top != null and .bounds.bottom != null and
            (($clickable == false) or (.clickable == true))
          )
        | {
            x: ((.bounds.left + .bounds.right) / 2),
            y: (if $anchor == "top-edge"
                then (.bounds.top + 12)
                elif $anchor == "top-quarter"
                then (.bounds.top + ((.bounds.bottom - .bounds.top) / 4))
                else ((.bounds.top + .bounds.bottom) / 2)
                end)
          }
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
  printf 'Không tìm thấy element %q trên %s sau %d lần thử.\n' "$filter" "$DEVICE_ID" "$max_attempts" >&2
  return 1
}

tap_address_bar() {
  tap_first_element 'Search or enter website' false ||
    tap_first_element 'Search or enter web' false ||
    tap_first_element 'Address and Search' false ||
    tap_first_element 'Address and search bar' false ||
    tap_first_element 'Smart Search' false
}

ios_safari_address_center=''

find_ios_safari_address_center() {
  local arguments candidate
  arguments="$(jq -cn --arg device "$DEVICE_ID" \
    '{deviceId:$device, filter:"Address", maxResults:10}')"
  call_tool mobile_list_elements_on_screen "$arguments"
  candidate="$(jq -cer '
    [
      .result.structuredContent.elements[]
      | select(
          .clickable == true and
          ((.contentDescription // "") == "Address" or (.text // "") == "Address") and
          .bounds.left != null and .bounds.right != null and
          .bounds.top != null and .bounds.bottom != null
        )
      | {x: ((.bounds.left + .bounds.right) / 2), y: ((.bounds.top + .bounds.bottom) / 2)}
    ] | first?
  ' <<<"$last_response" 2>/dev/null || true)"
  [[ -n "$candidate" && "$candidate" != null ]] || {
    printf 'Không tìm thấy TextField Address có thể tương tác trên %s.\n' "$DEVICE_ID" >&2
    return 1
  }
  ios_safari_address_center="$candidate"
}

search_for_chatgpt() {
  if [[ "$APP_ID" == "com.apple.mobilesafari" ]]; then
    local x y
    find_ios_safari_address_center
    x="$(jq -r '.x | round' <<<"$ios_safari_address_center")"
    y="$(jq -r '.y | round' <<<"$ios_safari_address_center")"
    # XCTest starts a new process for each MCP tool call. Keep the tap and type
    # in one batch so Safari retains keyboard focus.
    call_tool mobile_execute_ui_actions "$(jq -cn \
      --arg device "$DEVICE_ID" \
      --argjson x "$x" \
      --argjson y "$y" \
      '[{type:"tap", x:$x, y:$y}, {type:"type", text:"chatgpt.com"}] | {deviceId:$device, actions:.}')"
    tap_first_element 'Go' false 3 ||
      tap_first_element 'Search' false 3 ||
      tap_first_element 'Return' false 3
  else
    tap_address_bar
    call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, text:"chatgpt.com"}')"
    tap_first_element 'Go' false 3 || tap_first_element 'Search' false 3
  fi
}

open_ios_safari_new_tab() {
  # Use Safari Private before creating the tab so ChatGPT cannot restore a
  # draft from the regular tab group. All controls are located through the
  # accessibility tree; no screen coordinates are assumed.
  tap_first_element 'Tabs' true 1
  sleep "$SETTLE_SECONDS"
  tap_first_element 'Private' true 1 ||
    tap_first_element 'Private Browsing' true 1
  sleep "$SETTLE_SECONDS"
  # iOS Tab Overview exposes its + control as "Add", not "New Tab".
  tap_first_element 'Add' true 1
}

warm_up_ios_xctest_runner() {
  [[ "$APP_ID" == "com.apple.mobilesafari" ]] || return 0
  # The first filtered lookup may have to build the XCTest runner. Do that
  # once with a cold-start budget; subsequent UI lookups use the normal budget
  # and hit the shared, complete DerivedData cache without rebuilding.
  call_tool mobile_list_elements_on_screen "$(jq -cn \
    --arg device "$DEVICE_ID" \
    --argjson timeout "$XCTEST_WARMUP_TIMEOUT_MS" \
    '{deviceId:$device, filter:"Tabs", maxResults:1, timeoutMs:$timeout}')"
}

tap_chatgpt_search_result() {
  require_element 'Chat with ChatGPT'
}

dismiss_chatgpt_cookie_banner_if_present() {
  # ChatGPT can render this consent sheet above the composer on a fresh Safari
  # profile. It must be dismissed before looking up the message field.
  tap_first_element 'Reject non-essential' true 1 || true
}

require_element() {
  local filter="$1" arguments candidate attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    '{deviceId:$device, filter:$filter, maxResults:10}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    candidate="$(jq -cer '
      .result.structuredContent.elements[]
      | select(.bounds.left != null and .bounds.right != null and .bounds.top != null and .bounds.bottom != null)
    ' <<<"$last_response" 2>/dev/null || true)"
    [[ -n "$candidate" ]] && return 0
    sleep 0.25
  done
  printf 'Không xác nhận được element %q trên %s sau tối đa 3 giây.\n' "$filter" "$DEVICE_ID" >&2
  return 1
}

require_sent_message() {
  local arguments candidate attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "hello" \
    '{deviceId:$device, filter:$filter, maxResults:20}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    candidate="$(jq -cer '
      .result.structuredContent.elements[]
      | select((.role // "") == "text")
      | select((.text // "") | ascii_downcase | contains("hello"))
      | select(.bounds.left != null and .bounds.right != null and .bounds.top != null and .bounds.bottom != null)
    ' <<<"$last_response" 2>/dev/null || true)"
    [[ -n "$candidate" ]] && return 0
    sleep 0.25
  done
  printf 'Không xác nhận được tin nhắn hello đã gửi trên %s sau tối đa 3 giây.\n' "$DEVICE_ID" >&2
  return 1
}

dismiss_translate_popup_if_present() {
  local popup candidate x y
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
  # The stdio server can otherwise remain blocked on its FIFO after this
  # script exits; it is a child created exclusively for this run.
  kill "$MCP_SERVER_PID" 2>/dev/null || true
  wait "$MCP_SERVER_PID" 2>/dev/null || true
  rm -f "$INPUT_PIPE" "$OUTPUT_PIPE"
  rmdir "$SESSION_DIR" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT

# MCP lifecycle.
send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"device-robot-safari-example","version":"1.0"}}}'
receive 1
send '{"jsonrpc":"2.0","method":"notifications/initialized"}'
next_id=2

call_tool mobile_list_available_devices '{}'
call_tool mobile_start_screen_recording "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')"
recording_started=true
call_tool mobile_launch_app "$(jq -cn --arg device "$DEVICE_ID" --arg app "$APP_ID" '{deviceId:$device, appId:$app}')"
sleep "$SETTLE_SECONDS"

if [[ "$APP_ID" == "com.apple.mobilesafari" ]]; then
  warm_up_ios_xctest_runner
  open_ios_safari_new_tab
else
  # Safari trên macOS có thể đã mở một tab; nếu nút New Tab chưa xuất hiện thì
  # dùng tab đó.
  tap_first_element 'New Tab' true || printf 'Không tìm thấy New Tab; dùng tab hiện tại.\n' >&2
fi
search_for_chatgpt
sleep "$SETTLE_SECONDS"
dismiss_translate_popup_if_present

# UI web: đã mở trực tiếp chatgpt.com, không tìm kết quả tìm kiếm trung gian.
tap_chatgpt_search_result
sleep "$SETTLE_SECONDS"
dismiss_chatgpt_cookie_banner_if_present
tap_first_element 'Chat with ChatGPT' false ||
  tap_first_element 'Ask ChatGPT' false ||
  tap_first_element 'Message ChatGPT' false ||
  tap_first_element 'Message' false
# Safari/ChatGPT can restore the previous draft in a new tab. Remove any
# restored characters before typing the deterministic test message.
clear_chatgpt_draft() {
  local delete_keys
  delete_keys=$'\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b\b'
  call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" --arg text "$delete_keys" \
    '{deviceId:$device, text:$text}')" || true
}
clear_chatgpt_draft
call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, text:"hello"}')"
tap_first_element 'Send message' true 12 top-edge ||
  tap_first_element 'Send prompt' true ||
  tap_first_element 'Send' true
# Sau khi gửi, tin nhắn của người dùng phải xuất hiện trong conversation.
require_sent_message
sleep "$SETTLE_SECONDS"

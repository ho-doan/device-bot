#!/usr/bin/env bash
# Kịch bản MCP Safari: tìm ChatGPT, nhập "hello", gửi và quay màn hình.
# Dùng khi thiết bị không có Chrome. Cần jq để đọc structuredContent trả về từ MCP.
#
# Chạy macOS: ./server/examples/example_safari.sh
# Chạy iOS Simulator: DEVICE_ID=<simulator-id> ./server/examples/example_safari.sh ios-sim
# Chạy iPhone thật: DEVICE_ID=<iphone-udid> MOBILE_MCP_IOS_AGENT_TOKEN=<token> \
#   ./server/examples/example_safari.sh ios-real

set -euo pipefail

# An inherited SDKROOT may point at an SDK removed by an Xcode update. Let
# xcrun/xcodebuild select the active Xcode SDK for this standalone scenario.
unset SDKROOT

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SERVER_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
readonly REQUESTED_TARGET="${1:-${DEVICE_PLATFORM:-auto}}"
[[ "$#" -le 1 ]] || {
  printf 'Dùng: %s [macos|ios-sim|ios-real].\n' "$0" >&2
  exit 2
}

requested_app_id="${APP_ID:-}"
target="$REQUESTED_TARGET"
if [[ "$target" == auto ]]; then
  if [[ "$requested_app_id" == "com.apple.mobilesafari" ]]; then
    if [[ -n "${MOBILE_MCP_IOS_DEVICE:-}" || -n "${MOBILE_MCP_IOS_AGENT_BIND:-}" ]]; then
      target=ios-real
    else
      target=ios-sim
    fi
  else
    target=macos
  fi
fi
case "$target" in
  macos|ios-sim|ios-real) ;;
  *)
    printf 'Target không hợp lệ %q; chọn macos, ios-sim hoặc ios-real.\n' "$target" >&2
    exit 2
    ;;
esac
readonly TARGET="$target"

if [[ "$TARGET" == "ios-real" ]]; then
  readonly DEVICE_ID="${DEVICE_ID:-${MOBILE_MCP_IOS_DEVICE:-}}"
  [[ -n "$DEVICE_ID" ]] || {
    printf 'ios-real cần DEVICE_ID hoặc MOBILE_MCP_IOS_DEVICE là UDID iPhone.\n' >&2
    exit 2
  }
  readonly APP_ID="${APP_ID:-com.apple.mobilesafari}"
else
  readonly DEVICE_ID="${DEVICE_ID:-macos-host}"
  if [[ "$TARGET" == "ios-sim" ]]; then
    readonly APP_ID="${APP_ID:-com.apple.mobilesafari}"
  else
    readonly APP_ID="${APP_ID:-Safari}"
  fi
fi
# `Safari` is the macOS application name. Override APP_ID with the platform's
# Safari identifier (for example, com.apple.mobilesafari on an iOS Simulator).
readonly SETTLE_SECONDS="${SETTLE_SECONDS:-3}"
readonly RECORD_SCREEN_MODE="${RECORD_SCREEN:-auto}"
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
[[ "$RECORD_SCREEN_MODE" =~ ^(auto|0|1)$ ]] || {
  printf 'RECORD_SCREEN phải là auto, 0 hoặc 1.\n' >&2
  exit 2
}
if [[ "$TARGET" == "ios-real" ]]; then
  [[ -n "${MOBILE_MCP_IOS_AGENT_TOKEN:-}" ]] || {
    printf 'ios-real cần MOBILE_MCP_IOS_AGENT_TOKEN giống token của ios_xctest_agent.sh start.\n' >&2
    exit 2
  }
  if [[ "${MOBILE_MCP_IOS_AGENT_BIND:-}" =~ ^(127\.0\.0\.1|localhost)(:|$) ]]; then
    printf 'MOBILE_MCP_IOS_AGENT_BIND của ios-real phải là địa chỉ LAN, không phải loopback.\n' >&2
    exit 2
  fi
fi

record_screen=1
if [[ "$RECORD_SCREEN_MODE" == 0 ]]; then
  record_screen=0
fi

readonly SESSION_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-robot-mcp.XXXXXX")"
readonly INPUT_PIPE="$SESSION_DIR/input"
readonly OUTPUT_PIPE="$SESSION_DIR/output"
# Keep the XCTest products in the workspace target directory so separate MCP
# processes and later example/test runs share one machine-local build cache.
readonly XCTEST_DERIVED_DATA_DIR="${MOBILE_MCP_IOS_XCTEST_DERIVED_DATA:-$SERVER_DIR/target/mobile-mcp-ios-xctest-derived-data-persistent-agent}"
export MOBILE_MCP_IOS_XCTEST_DERIVED_DATA="$XCTEST_DERIVED_DATA_DIR"
# The real-device XCTest agent runs on the iPhone and must connect back to a
# stable, LAN-reachable bridge. Simulator runs can keep an ephemeral loopback
# port because their runner is launched by the host.
if [[ "$TARGET" == "ios-real" ]]; then
  export MOBILE_MCP_IOS_AGENT_BIND="${MOBILE_MCP_IOS_AGENT_BIND:-0.0.0.0:${MOBILE_MCP_IOS_AGENT_PORT:-18787}}"
else
  export MOBILE_MCP_IOS_AGENT_BIND="${MOBILE_MCP_IOS_AGENT_BIND:-127.0.0.1:0}"
fi
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

wait_for_real_ios_agent() {
  local arguments attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')"
  for ((attempt = 1; attempt <= 30; attempt++)); do
    if call_tool mobile_get_ui_runner_status "$arguments" &&
      [[ "$(jq -r '.result.structuredContent.ready // false' <<<"$last_response")" == true ]]; then
      return 0
    fi
    sleep 1
  done
  printf 'iOS XCTest agent chưa sẵn sàng sau 30 giây trên %s.\n' "$DEVICE_ID" >&2
  return 1
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
  local arguments candidate address_filter="Address"
  if [[ "$TARGET" == "ios-real" ]]; then
    address_filter='TabBarItemTitle'
  fi
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$address_filter" \
    '{deviceId:$device, filter:$filter, maxResults:10}')"
  call_tool mobile_list_elements_on_screen "$arguments"
  candidate="$(jq -cer '
    [
      .result.structuredContent.elements[]
      | select(
          .clickable == true and
          (
            ((.resourceId // "") == "TabBarItemTitle") or
            ((.contentDescription // "") | ascii_downcase | IN("address")) or
            ((.text // "") | ascii_downcase | IN("address"))
          ) and
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
      tap_first_element 'Return' false 3 ||
      tap_first_element 'Done' false 3
  else
    tap_address_bar
    call_tool mobile_type_keys "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device, text:"chatgpt.com"}')"
    tap_first_element 'Go' false 3 ||
      tap_first_element 'Search' false 3 ||
      tap_first_element 'Return' false 3
  fi
}

tap_accessibility_element() {
  local filter="$1" max_attempts="${2:-12}" arguments attempt
  if [[ "$TARGET" == "ios-real" ]]; then
    local real_filter
    case "$filter" in
      Tabs) real_filter='TabOverviewButton' ;;
      Private) real_filter='Private, Tab Group' ;;
      Done) real_filter='DoneButton' ;;
      Add) real_filter='NewTabButton' ;;
      *) real_filter="$filter" ;;
    esac
    arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$real_filter" \
      '{deviceId:$device, actions:[{type:"tap_element", filter:$filter}]}')"
    if call_tool mobile_execute_ui_actions "$arguments"; then
      return 0
    fi
    case "$filter" in
      Tabs)
        tap_first_element 'TabOverviewButton' true "$max_attempts" ||
          tap_first_element 'Tab' true "$max_attempts" ||
          tap_first_element 'Tabs' true "$max_attempts"
        ;;
      Private)
        tap_first_element 'Private, Tab Group' true "$max_attempts" ||
          tap_first_element 'CapsuleTabGroup' true "$max_attempts" ||
          tap_first_element 'Private' true "$max_attempts" ||
          tap_first_element 'Private Browsing' true "$max_attempts"
        ;;
      Done)
        tap_first_element 'DoneButton' false "$max_attempts" ||
          tap_first_element 'Done' false "$max_attempts" ||
          tap_first_element 'Close' false "$max_attempts"
        ;;
      Add)
        tap_first_element 'NewTabButton' true "$max_attempts" ||
          tap_first_element 'Add' true "$max_attempts" ||
          tap_first_element 'New Tab' true "$max_attempts"
        ;;
      *)
        tap_first_element "$filter" true "$max_attempts"
        ;;
    esac
    return
  fi
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    '{deviceId:$device, actions:[{type:"tap_element", filter:$filter}]}')"
  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
    if call_tool mobile_execute_ui_actions "$arguments"; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

tap_send_button() {
  tap_accessibility_element 'Send message' 12 ||
    tap_accessibility_element 'Send prompt' 12 ||
    tap_accessibility_element 'Send' 12 ||
    tap_first_element 'Send message' true 12 top-edge ||
    tap_first_element 'Send prompt' true ||
    tap_first_element 'Send' true
}

has_accessibility_element() {
  local filter="$1" arguments
  arguments="$(jq -cn --arg device "$DEVICE_ID" --arg filter "$filter" \
    '{deviceId:$device, filter:$filter, maxResults:1}')"
  call_tool mobile_list_elements_on_screen "$arguments" >/dev/null &&
    jq -e --arg filter "$filter" '
      (.result.structuredContent.elements // [])[]
      | ((.resourceId // "") == $filter or
         (.contentDescription // "") == $filter or
         (.text // "") == $filter)
    ' <<<"$last_response" >/dev/null
}

open_ios_safari_new_tab() {
  # Use Safari Private before creating the tab so ChatGPT cannot restore a
  # draft from the regular tab group. All controls are located through the
  # accessibility tree; no screen coordinates are assumed.
  if [[ "$TARGET" == "ios-real" ]]; then
    if ! has_accessibility_element 'TabOverview'; then
      tap_accessibility_element 'Tabs' 12
    fi
  else
    tap_accessibility_element 'Tabs' 12
  fi
  sleep "$SETTLE_SECONDS"
  tap_accessibility_element 'Private' 12 ||
    tap_accessibility_element 'Private Browsing' 12
  sleep "$SETTLE_SECONDS"
  # Selecting the Private tab group opens the Tab Groups sheet. Confirm the
  # selected group before looking for the new-tab control.
  tap_accessibility_element 'Done' 12 ||
    tap_accessibility_element 'Close' 12
  sleep "$SETTLE_SECONDS"
  # iOS Tab Overview exposes its + control as "Add", not "New Tab".
  tap_accessibility_element 'Add' 20
}

warm_up_ios_xctest_runner() {
  [[ "$APP_ID" == "com.apple.mobilesafari" ]] || return 0
  # The persistent agent is already built. Avoid a full Safari button dump
  # here because the private-tab overview exposes modern accessibility nodes
  # that can block XCTest's legacy indexed query.
  return 0
}

tap_chatgpt_search_result() {
  require_element 'Chat with ChatGPT'
}

dismiss_chatgpt_cookie_banner_if_present() {
  # ChatGPT can render this consent sheet above the composer on a fresh Safari
  # profile. It must be dismissed before looking up the message field.
  tap_first_element 'Reject non-essential' true 12 || true
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
  local arguments composer_top input_has_hello candidate attempt
  arguments="$(jq -cn --arg device "$DEVICE_ID" \
    '{deviceId:$device, maxResults:100}')"
  for ((attempt = 1; attempt <= 12; attempt++)); do
    call_tool mobile_list_elements_on_screen "$arguments"
    input_has_hello="$(jq -r '
      [
        .result.structuredContent.elements[]?
        | select((.className // .role // "") == "textField")
        | [(.text // ""), (.contentDescription // ""), (.value // "")]
        | join(" ")
      ]
      | join(" ")
      | ascii_downcase
      | contains("hello")
    ' <<<"$last_response")"
    composer_top="$(jq -r '
      [
        .result.structuredContent.elements[]?
        | select((.className // .role // "") == "textField")
        | .bounds.top
        | numbers
      ]
      | min // null
    ' <<<"$last_response")"
    if [[ "${DEVICE_ROBOT_DEBUG:-}" == "1" ]]; then
      jq -c --arg input_has_hello "$input_has_hello" --arg composer_top "$composer_top" \
        '{input_has_hello:$input_has_hello, composer_top:$composer_top, elements:.result.structuredContent.elements}' \
        <<<"$last_response" >&2
    fi
    candidate=''
    if [[ "$input_has_hello" != true ]]; then
      candidate="$(jq -cer --argjson composer_top "$composer_top" '
        .result.structuredContent.elements[]
        | select((.className // .role // "") != "textField")
        | select(
            [(.text // ""), (.contentDescription // ""), (.value // "")]
            | join(" ")
            | ascii_downcase
            | contains("hello")
          )
        | select(.bounds.left != null and .bounds.right != null and .bounds.top != null and .bounds.bottom != null)
        | select($composer_top == null or .bounds.bottom <= $composer_top)
      ' <<<"$last_response" 2>/dev/null || true)"
    fi
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

if [[ "$TARGET" == "ios-real" ]]; then
  wait_for_real_ios_agent
fi
call_tool mobile_list_available_devices '{}'
if [[ "$record_screen" == 1 ]]; then
  call_tool mobile_start_screen_recording "$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')"
  recording_started=true
fi
if [[ "$TARGET" == "ios-real" ]]; then
  runner_status_arguments="$(jq -cn --arg device "$DEVICE_ID" '{deviceId:$device}')"
  call_tool mobile_get_ui_runner_status "$runner_status_arguments"
  if [[ "$(jq -r '.result.structuredContent.ready // false' <<<"$last_response")" != true ]]; then
    jq -r '.result.structuredContent.detail // "iOS XCTest agent chưa sẵn sàng"' <<<"$last_response" >&2
    exit 1
  fi
fi
call_tool mobile_launch_app "$(jq -cn --arg device "$DEVICE_ID" --arg app "$APP_ID" '{deviceId:$device, appId:$app}')"
sleep "$SETTLE_SECONDS"

if [[ "$TARGET" == "ios-sim" || "$TARGET" == "ios-real" ]]; then
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
dismiss_chatgpt_cookie_banner_if_present

# UI web: đã mở trực tiếp chatgpt.com, không tìm kết quả tìm kiếm trung gian.
tap_chatgpt_search_result
sleep "$SETTLE_SECONDS"
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
tap_send_button
# Sau khi gửi, tin nhắn của người dùng phải xuất hiện trong conversation.
require_sent_message
sleep "$SETTLE_SECONDS"

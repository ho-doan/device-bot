#!/usr/bin/env bash
# Shared Orbit scenario helpers. Source from an it-0*.sh entrypoint.
# All UI actions use the installed MCP server; Android/Simulator cold-stop uses CLI.
set -euo pipefail
: "${CASE_ID:?Set CASE_ID before sourcing support/mcp.sh}"
: "${DEVICE_ID:?Set DEVICE_ID to the Android serial or iOS device/Simulator UDID}"
readonly PLATFORM="${PLATFORM:-android}"
case "$PLATFORM" in
  android) APP_ID="${APP_ID:-com.example.flutter_app}"; PLATFORM_COMMAND="${ANDROID_ADB_PATH:-adb}" ;;
  ios|ios-sim) APP_ID="${APP_ID:-com.example.flutter-myapp}"; PLATFORM_COMMAND="${XCRUN_PATH:-xcrun}"; unset SDKROOT ;;
  *) printf 'PLATFORM must be android, ios or ios-sim\n' >&2; exit 2 ;;
esac
readonly APP_ID PLATFORM_COMMAND
readonly RPC_TIMEOUT="${RPC_TIMEOUT:-180}"
[[ "$RPC_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { printf 'Invalid RPC_TIMEOUT\n' >&2; exit 2; }
for dependency in jq "${DEVICE_BOT_BIN:-device-bot}" "$PLATFORM_COMMAND"; do
  command -v "$dependency" >/dev/null || { printf 'Missing: %s\n' "$dependency" >&2; exit 2; }
done
readonly ARTIFACT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-bot-${CASE_ID}.XXXXXX")"
printf 'Artifacts: %s\n' "$ARTIFACT_DIR"
mkfifo "$ARTIFACT_DIR/input" "$ARTIFACT_DIR/output"
"${DEVICE_BOT_BIN:-device-bot}" <"$ARTIFACT_DIR/input" >"$ARTIFACT_DIR/output" &
readonly MCP_PID=$!
exec 3>"$ARTIFACT_DIR/input"
exec 4<"$ARTIFACT_DIR/output"
next_id=2
last_response=''
initialized=false
limitations=0
query_count=0
email_value=''
password_value=''
FOUND_ELEMENT=''

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

scroll_screen() {
  local direction="${1:-down}"
  local dimensions
  call_tool mobile_list_elements_on_screen "$(jq -cn --arg d "$DEVICE_ID" \
    '{deviceId:$d,maxResults:100,timeoutMs:60000}')"
  if [[ "$PLATFORM" == ios* ]]; then
    # Use the app's largest scroll viewport, not keyboard/accessory buttons.
    # XCTest reports these bounds in the same points used by input actions.
    dimensions="$(jq -ce '[.result.structuredContent.elements[] |
      select(.className=="scrollView") | .bounds |
      select(.right>.left and .bottom>.top)] |
      max_by((.right-.left)*(.bottom-.top)) |
      {x:.left,y:.top,w:(.right-.left),h:(.bottom-.top)} |
      select(.w>0 and .h>0)' <<<"$last_response")"
  else
    # The IME shrinks the app viewport. A screenshot-sized swipe would hit the
    # keyboard instead of the form; use the native ScrollView bounds.
    dimensions="$(jq -ce '[.result.structuredContent.elements[] |
      select(.className|test("ScrollView")) | .bounds | select(.bottom>.top)] |
      first | {w:.right,h:.bottom} | select(.w>0 and .h>0)' <<<"$last_response")"
  fi
  call_tool mobile_swipe_on_screen "$(jq -cn --arg d "$DEVICE_ID" --arg direction "$direction" --argjson s "$dimensions" \
    '{deviceId:$d,startX:(($s.x//0)+$s.w*0.5),startY:(($s.y//0)+$s.h*(if $direction=="down" then 0.72 else 0.4 end)),
      endX:(($s.x//0)+$s.w*0.5),endY:(($s.y//0)+$s.h*(if $direction=="down" then 0.4 else 0.72 end)),durationMs:350}')"
}
scroll_down() { scroll_screen down; }

query_elements() {
  local label="${1:-}" element_type="${2:-}" max_results="${3:-100}"
  call_tool mobile_list_elements_on_screen "$(jq -cn \
    --arg d "$DEVICE_ID" --arg f "$label" --arg t "$element_type" --argjson n "$max_results" \
    '{deviceId:$d,maxResults:$n,timeoutMs:60000}
      + (if $f=="" then {} else {filter:$f} end)
      + (if $t=="" then {} else {elementType:$t} end)')"
  query_count=$((query_count + 1))
  jq '.result.structuredContent.elements' <<<"$last_response" >"$ARTIFACT_DIR/last-query.json"
  cp "$ARTIFACT_DIR/last-query.json" "$ARTIFACT_DIR/query-${query_count}.json"
}

# maxResults is one above the expected count so an extra field is not truncated away.
assert_text_field_count() {
  local expected="$1" count
  query_elements '' textField $((expected + 1))
  count="$(jq 'length' "$ARTIFACT_DIR/last-query.json")"
  if [[ "$count" != "$expected" ]]; then
    printf 'FAIL: expected %s text field(s), found %s\n' "$expected" "$count" >&2
    return 1
  fi
  printf 'PASS: %s text field\n' "$expected"
}

press_ime_action() {
  local action="$1"
  call_tool mobile_press_ime_action "$(jq -cn --arg d "$DEVICE_ID" --arg a "$action" \
    '{deviceId:$d,action:$a}')"
  printf 'PASS: IME %s\n' "$action"
}

assert_focused_field() {
  local label="$1"
  find_element "$label" false true field
  if ! jq -e '.focused == true' <<<"$FOUND_ELEMENT" >/dev/null; then
    printf 'FAIL: %s is not focused\n' "$label" >&2
    return 1
  fi
  printf 'PASS: %s focused\n' "$label"
}

assert_checked() {
  local label="$1" expected="$2" attempt found native_type=checkBox
  # Orbit's Flutter CheckboxListTile is exposed by XCTest as a native switch.
  [[ "$PLATFORM" == ios* ]] && native_type=switch
  for ((attempt=0; attempt<12; attempt++)); do
    query_elements "$label" "$native_type"
    found="$(jq -r --arg label "$label" '
      [.[] | select(((.text // "") + "\n" + (.contentDescription // "")) | contains($label))] | length
    ' "$ARTIFACT_DIR/last-query.json")"
    if [[ "$found" == 1 ]] && jq -e --arg label "$label" --argjson expected "$expected" '
      [.[] | select(((.text // "") + "\n" + (.contentDescription // "")) | contains($label))]
      | .[0].checked == $expected
    ' "$ARTIFACT_DIR/last-query.json" >/dev/null; then
      printf 'PASS: %s checked=%s\n' "$label" "$expected"
      return 0
    fi
    # A visible checkbox with another state is a real mismatch, not a scroll miss.
    if [[ "$found" != 0 ]]; then
      printf 'FAIL: %s checked=%s was not observed\n' "$label" "$expected" >&2
      return 1
    fi
    if ((attempt >= 1 && attempt < 11)); then
      if ((attempt < 5)); then scroll_screen down; else scroll_screen up; fi
    fi
    sleep 0.5
  done
  printf 'FAIL: %s checked=%s was not observed\n' "$label" "$expected" >&2
  return 1
}

# kind=field restricts matching to native text fields, avoiding hints/demo text.
# kind=any allows tapping inert headings to move focus without a navigation action.
find_element() {
  local label="$1" tap="${2:-false}" scroll="${3:-false}" kind="${4:-label}" attempt matches count
  local previous_target='' current_target
  for ((attempt=0; attempt<12; attempt++)); do
    if [[ "$kind" == field && "$PLATFORM" == ios* ]]; then
      query_elements "$label" textField
    elif [[ "$kind" == field ]]; then
      query_elements ''
    else
      query_elements "$label"
    fi
    matches="$(jq -c --arg label "$label" --arg kind "$kind" --arg platform "$PLATFORM" --argjson tap "$tap" '
      # Android hints are not normalized. Identify the password field by the
      # Show/Hide password button in the same row; email is the other textbox.
      . as $all |
      [$all[] | select((.contentDescription // "") | test("^(Show|Hide) password$";"i")) |
        ((.bounds.top+.bounds.bottom)/2)] as $passwordRows |
      (if $kind=="field" and ($platform|startswith("ios")) then
         [.[] | select((.contentDescription // "" | ascii_downcase)==($label|ascii_downcase))]
       elif $kind=="field" then
         [.[] | select(.className|test("EditText|textField";"i")) |
          .bounds as $b |
          select((any($passwordRows[]; . >= $b.top and . <= $b.bottom)) == ($label=="Password"))]
       else . end) |
      [.[] | select(($kind=="field" or ([.text // "",.contentDescription // ""] |
        any((ascii_downcase == ($label|ascii_downcase)) or
            (split("\n") | map(ascii_downcase) | index($label|ascii_downcase) != null)
            )))
        and ($kind!="field" or (.className|test("EditText|textField";"i")))
        and .bounds.left >= 0 and .bounds.top >= 0
        and .bounds.right > .bounds.left and .bounds.bottom > .bounds.top
        and (($tap | not) or (.enabled and (.clickable or $kind=="any"))))] |
      if $tap and ($platform|startswith("ios")) then
        . as $all | [.[] | select(.className|test("^(button|textField|switch|checkBox|menuItem|link|radio|slider)$"))] as $controls |
        if ($controls|length)>0 then $controls else $all end
      else . end' "$ARTIFACT_DIR/last-query.json")"
    count="$(jq length <<<"$matches")"
    if ((count > 0)); then
      FOUND_ELEMENT="$(jq -c '.[0]' <<<"$matches")"
      if [[ "$tap" == true ]]; then
        [[ "$count" == 1 ]] || { printf 'Ambiguous: %s\n' "$label" >&2; return 1; }
        if [[ "$PLATFORM" == ios* ]]; then
          # Flutter publishes intermediate frames during keyboard/scroll
          # animations. Require the same target bounds in consecutive queries.
          current_target="$(jq -c '{bounds,className,contentDescription}' <<<"$FOUND_ELEMENT")"
          if [[ "$current_target" != "$previous_target" ]]; then
            previous_target="$current_target"
            sleep 0.25
            continue
          fi
        fi
        if [[ "$PLATFORM" == ios* ]] && jq -e '.className == "textField" or .className == "menuItem"' <<<"$FOUND_ELEMENT" >/dev/null; then
          # A partially covered element may be hittable while its center is not.
          # Let XCTest select the actual hit point instead of tapping the IME.
          call_tool mobile_execute_ui_actions "$(jq -cn --arg d "$DEVICE_ID" --arg f "$label" \
            '{deviceId:$d,actions:[{type:"tap_element",filter:$f}]}')"
        else
          # Use verified visible bounds for ordinary Flutter controls. Native
          # switch/button tap may unexpectedly scroll their Flutter container.
          call_tool mobile_click_on_screen_at_coordinates "$(jq -c --arg d "$DEVICE_ID" '
            .[0].bounds | {deviceId:$d,x:((.left+.right)/2),y:((.top+.bottom)/2)}' <<<"$matches")"
        fi
      fi
      printf 'PASS: %s\n' "$label"; return 0
    fi
    previous_target=''
    if [[ "$scroll" == true ]] && ((attempt >= 1 && attempt < 11)); then
      # Search both directions: validation/keyboard changes can scroll the form.
      if ((attempt < 5)); then scroll_screen down; else scroll_screen up; fi
    fi
    sleep 0.5
  done
  printf 'FAIL: Missing %s (see %s)\n' "$label" "$ARTIFACT_DIR" >&2; return 1
}

tap_control() { find_element "$1" true true; }
assert_text() { find_element "$1" false true; }

tap_account_avatar() {
  query_elements 'Your account'
  if jq -e 'any(.[]; .clickable and .enabled)' "$ARTIFACT_DIR/last-query.json" >/dev/null; then
    tap_control 'Your account'
  else
    # Android Flutter merges the tooltip into the child avatar label instead.
    tap_control 'AL'
  fi
}

assert_absent() {
  local label="$1"
  query_elements "$label"
  if ! jq -e 'length==0' "$ARTIFACT_DIR/last-query.json" >/dev/null; then
    printf 'FAIL: Unexpected element %s\n' "$label" >&2; return 1
  fi
  printf 'PASS: absent %s\n' "$label"
}

type_text() {
  call_tool mobile_type_keys "$(jq -cn --arg d "$DEVICE_ID" --arg t "$1" '{deviceId:$d,text:$t}')"
}

fill_field() {
  local label="$1" value="$2" previous bounds restore_hidden=false
  if [[ "$label" == 'Email address' ]]; then previous="$email_value"; else previous="$password_value"; fi
  find_element "$label" true true field
  if [[ -n "$previous" ]]; then
    if [[ "$label" == Password ]]; then
      # Masked Flutter fields can select the whole value without exposing a
      # Select all menu. Reveal the demo password for deterministic native editing.
      query_elements 'Show password'
      if jq -e 'any(.[]; .clickable and .enabled)' "$ARTIFACT_DIR/last-query.json" >/dev/null; then
        tap_control 'Show password'
        restore_hidden=true
        find_element "$label" true true field
      fi
    fi
    if [[ "$PLATFORM" == ios* ]]; then
      # Native tap may scroll the field while presenting the keyboard.
      find_element "$label" false true field
    fi
    # Long-press the native field, then use its Select all editing action.
    # Android mobile_type_keys rejects control characters, so do not send \b.
    bounds="$(jq -c '.bounds' <<<"$FOUND_ELEMENT")"
    call_tool mobile_swipe_on_screen "$(jq -cn --arg d "$DEVICE_ID" --argjson b "$bounds" '
      {deviceId:$d,startX:($b.left+($b.right-$b.left)*0.25),startY:(($b.top+$b.bottom)/2),
       endX:($b.left+($b.right-$b.left)*0.25),endY:(($b.top+$b.bottom)/2),durationMs:1000}')"
    query_elements 'Select all'
    if jq -e 'any(.[]; .clickable and .enabled)' "$ARTIFACT_DIR/last-query.json" >/dev/null; then
      tap_control 'Select all'
    else
      # A single-word value can already be fully selected by the long press.
      # Cut it and prove the native textbox is empty before typing a replacement.
      # If only part was selected, fail rather than append to the remaining text.
      tap_control 'Cut'
      find_element "$label" false true field
      if ! jq -e '.text==null or .text==""' <<<"$FOUND_ELEMENT" >/dev/null; then
        printf 'FAIL: Native selection did not clear the entire %s field\n' "$label" >&2; return 1
      fi
    fi
  fi
  type_text "$value"
  if [[ "$label" == 'Email address' ]]; then email_value="$value"; else password_value="$value"; fi
  if [[ "$restore_hidden" == true ]]; then
    assert_field_value 'Password' "$value"
    tap_control 'Hide password'
  fi
}

enter_credentials() {
  fill_field 'Email address' "$1"
  fill_field 'Password' "$2"
}

assert_field_value() {
  local label="$1" expected="$2"
  find_element "$label" false true field
  if ! jq -e --arg expected "$expected" '
    [.text // "", .contentDescription // ""] |
      any(. == $expected or (split("\n")|index($expected)!=null))
    ' <<<"$FOUND_ELEMENT" >/dev/null; then
    printf 'FAIL: Field %s does not expose the expected value\n' "$label" >&2; return 1
  fi
  printf 'PASS: %s value preserved\n' "$label"
}

note_limitation() {
  limitations=$((limitations + 1))
  printf 'NOT COVERED: %s\n' "$1" | tee -a "$ARTIFACT_DIR/limitations.txt"
}

finish_flow() {
  screenshot result
  if ((limitations > 0)); then
    printf 'PARTIAL: %s %s UI checks passed; %s coverage limitation(s).\n' "$CASE_ID" "$PLATFORM" "$limitations"
  else
    printf 'PASS: %s %s UI via MCP.\n' "$CASE_ID" "$PLATFORM"
  fi
}

cold_launch() {
  if [[ "$PLATFORM" == android ]]; then
    "$PLATFORM_COMMAND" -s "$DEVICE_ID" shell am force-stop "$APP_ID"
  elif [[ "$PLATFORM" == ios-sim ]]; then
    if ! "$PLATFORM_COMMAND" simctl terminate "$DEVICE_ID" "$APP_ID" 2>"$ARTIFACT_DIR/terminate.log"; then
      if ! grep -qi 'found nothing to terminate' "$ARTIFACT_DIR/terminate.log"; then
        cat "$ARTIFACT_DIR/terminate.log" >&2; return 1
      fi
    fi
  fi
  # On real iOS, the agent's XCUIApplication.launch() restarts the target app.
  email_value=''; password_value=''
  call_tool mobile_launch_app "$(jq -cn --arg d "$DEVICE_ID" --arg a "$APP_ID" '{deviceId:$d,appId:$a}')"
  assert_text 'Welcome back.'
}

sign_in_demo() {
  tap_control 'Try demo workspace'
  assert_text "${1:-A fresh start, Alex.}"
}

printf '%s\n' "$(jq -cn --arg name "orbit-$CASE_ID" '{jsonrpc:"2.0",id:1,method:"initialize",params:{protocolVersion:"2025-11-25",capabilities:{},clientInfo:{name:$name,version:"1.0"}}}')" >&3
receive 1
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&3
initialized=true
# The server has no terminate tool; Android/Simulator cold-stop uses the CLI.
if [[ -n "${APP_PATH:-${APK_PATH:-}}" ]]; then
  call_tool mobile_install_app "$(jq -cn --arg d "$DEVICE_ID" --arg p "${APP_PATH:-$APK_PATH}" '{deviceId:$d,apkPath:$p}')"
fi
cold_launch

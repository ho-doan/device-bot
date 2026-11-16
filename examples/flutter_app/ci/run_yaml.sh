#!/usr/bin/env bash
# CI setup and one native Rust invocation per verified YAML case.
# UI actions, recording, deadlines and reports belong to robot-flow.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
profiles="${ORBIT_PROFILES:-automation_test/targets-semantics.yaml}"
output="${YAML_ARTIFACT_DIR:-artifacts/yaml}"
case_timeout="${SCENARIO_TIMEOUT:-600}"
rpc_timeout="${RPC_TIMEOUT:-180}"
mkdir -p "$output"
output="$(cd -- "$output" && pwd)"
# Do not let results from a previous invocation satisfy this run's checks.
[[ -z "$(find "$output" -mindepth 1 -print -quit)" ]] || {
  printf 'Evidence already exists: %s; use a fresh YAML_ARTIFACT_DIR.\n' "$output" >&2; exit 2;
}
windows_state=''
setup_in_progress=true
# shellcheck disable=SC2329 # Invoked by the EXIT trap.
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$windows_state" ]]; then
    "${DEVICE_BOT_BIN:-device-bot}" project windows-fixture stop "$APP_ID" "$windows_state" || { status=1; setup_in_progress=true; }
  fi
  if ((status != 0)) && [[ "$setup_in_progress" == true ]]; then
    "${DEVICE_BOT_BIN:-device-bot}" project ensure-report "$output/setup-error"
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
: "${PLATFORM:?Set PLATFORM}" "${DEVICE_ID:?Set DEVICE_ID}" "${APP_ID:?Set APP_ID}"
export DEVICE_BOT_BIN="${DEVICE_BOT_BIN:-device-bot}"
bot="$(command -v "$DEVICE_BOT_BIN")"
runner_suffix=''
[[ "$bot" != *.exe ]] || runner_suffix=.exe
runner="${ROBOT_FLOW_BIN:-$(dirname -- "$bot")/robot-flow$runner_suffix}"
command -v "$runner" >/dev/null
command -v ffprobe >/dev/null
command -v ffmpeg >/dev/null
command -v jq >/dev/null
# Both tools must come from the same release; the old 0.1.3 runner predates these flows.
runner_version="$("$runner" --version)"
bot_version="$("$DEVICE_BOT_BIN" --version)"
[[ -n "$runner_version" && "$runner_version" == "$bot_version" ]] || {
  printf 'robot-flow and device-bot versions must match.\n' >&2; exit 2;
}
case "$PLATFORM" in
  android) command -v "${ANDROID_ADB_PATH:-adb}" >/dev/null ;;
  ios-sim) command -v "${XCRUN_PATH:-xcrun}" >/dev/null; unset SDKROOT ;;
  windows)
    command -v cygpath >/dev/null
    export ORBIT_APP_PATH="$APP_ID"
    ;;
  *) printf 'Unsupported PLATFORM: %s\n' "$PLATFORM" >&2; exit 2 ;;
esac
shopt -s nullglob
scenarios=(automation_test/it-0[1-9].yaml)
[[ ${#scenarios[@]} -eq 9 ]] || { printf 'Expected all nine verified Orbit YAML cases.\n' >&2; exit 2; }
# Compile every case before touching the device. Drafts never match this list.
"$runner" --dry-run --profiles "$profiles" --target "$PLATFORM" \
  --device "$DEVICE_ID" --app-id "$APP_ID" --artifacts "$output/preflight" \
  "${scenarios[@]}" > "$output/preflight.log" 2>&1
suite_status=0
printf '# Preliminary YAML automation results\n\nQA review: PENDING.\n\n' > "$output/QA_REVIEW.md"
for scenario in "${scenarios[@]}"; do
  name="$(basename "$scenario" .yaml)"
  directory="$output/$name"
  [[ ! -e "$directory" ]] || { printf 'Evidence already exists: %s; use a fresh YAML_ARTIFACT_DIR.\n' "$directory" >&2; exit 2; }
  mkdir -p "$directory"
  case "$PLATFORM" in
    android) "${ANDROID_ADB_PATH:-adb}" -s "$DEVICE_ID" shell am force-stop "$APP_ID" ;;
    ios-sim)
      if ! "${XCRUN_PATH:-xcrun}" simctl terminate "$DEVICE_ID" "$APP_ID" 2>"$directory/terminate.log"; then
        grep -qi 'found nothing to terminate' "$directory/terminate.log" || { cat "$directory/terminate.log" >&2; exit 1; }
      fi
      # Starting XCTest can reset the Simulator video source; warm it before recording.
      "$runner" --profiles "$profiles" --target "$PLATFORM" --device "$DEVICE_ID" \
        --app-id "$APP_ID" --confirm fixture_ready --rpc-timeout "$rpc_timeout" \
        --case-timeout "$case_timeout" --artifacts "$directory/warmup" \
        automation_test/support/warmup.yaml > "$directory/warmup.log" 2>&1
      "${XCRUN_PATH:-xcrun}" simctl terminate "$DEVICE_ID" "$APP_ID"
      ;;
    windows)
      windows_state="$(cygpath -w "$directory/windows-fixture.json")"
      "$DEVICE_BOT_BIN" project windows-fixture start "$APP_ID" "$windows_state"
      ;;
  esac
  printf 'Running %s through robot-flow; QA review remains pending.\n' "$name"
  case_status=0
  # Keep setup_in_progress true until a report is checked, including cancellation.
  if ! "$runner" --profiles "$profiles" --target "$PLATFORM" --device "$DEVICE_ID" \
    --app-id "$APP_ID" --confirm fixture_ready --rpc-timeout "$rpc_timeout" \
    --case-timeout "$case_timeout" --artifacts "$directory" "$scenario" \
    > "$directory/runner.log" 2>&1; then
    case_status=1
    cat "$directory/runner.log" >&2
  fi
  # Check that the actual recording decodes as video with a positive duration.
  reports=("$directory"/case-*/report.json)
  junits=("$directory"/case-*/junit.xml)
  if [[ ${#reports[@]} -ne 1 || ${#junits[@]} -ne 1 ]] ||
    ! jq -e '.dry_run == false and .error == null and
      (.limitations | type == "array") and all(.limitations[]; type == "string") and
      ((.status == "PASS" and (.limitations | length) == 0) or
       (.status == "PARTIAL" and (.limitations | length) > 0))' "${reports[0]}" >/dev/null; then
    case_status=1
  fi
  videos=("$directory"/case-*/video.mp4)
  if [[ ${#videos[@]} -ne 1 ]] || ! ffprobe -v error \
    -show_entries stream=codec_type:format=duration -of json "${videos[0]}" \
    > "$directory/video-probe.json" 2>"$directory/video-probe.log" ||
    ! jq -e 'any(.streams[]; .codec_type == "video") and ((.format.duration | tonumber) > 0)' \
      "$directory/video-probe.json" >/dev/null ||
    ! ffmpeg -v error -xerror -i "${videos[0]}" -map 0:v:0 -f null - \
      > "$directory/video-decode.log" 2>&1; then
    case_status=1
    "$DEVICE_BOT_BIN" project ensure-report "$directory/video-error"
  fi
  "$DEVICE_BOT_BIN" project ensure-report "$directory"
  if ((case_status)); then
    suite_status=1
    "$DEVICE_BOT_BIN" project ensure-report "$directory/ci-error"
    printf -- '- %s: ERROR (inspect runner/video logs)\n' "$name" >> "$output/QA_REVIEW.md"
  else
    jq -r --arg name "$name" '"- \($name): \(.status)", (.limitations[] | "  - Not covered: \(.)")' \
      "${reports[0]}" >> "$output/QA_REVIEW.md"
  fi
  if [[ -n "$windows_state" ]]; then
    "$DEVICE_BOT_BIN" project windows-fixture stop "$APP_ID" "$windows_state"
    windows_state=''
  fi
  setup_in_progress=true
done
setup_in_progress=false
exit "$suite_status"

#!/usr/bin/env bash
# Exercise the real CI shell entrypoint with a process fixture, never a device.
set -euo pipefail
app="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/app/ci" "$root/app/automation_test/support" "$root/bin"
cp "$app/ci/run_yaml.sh" "$root/app/ci/"
cp "$app/automation_test/support/ensure_yaml_report.sh" "$root/app/automation_test/support/"
cp "$app/automation_test/it-0"*.yaml "$root/app/automation_test/"
# An unverified draft and an unrelated YAML must never enter the CI case list.
touch "$root/app/automation_test/it-10.yaml.draft" "$root/app/automation_test/other.yaml"
ffmpeg -v error -f lavfi -i color=size=16x16:rate=5 -t 0.4 -c:v mpeg4 "$root/video.mp4"
cat > "$root/bin/device-bot" <<'SH'
#!/bin/sh
echo 0.1.24
SH
cat > "$root/bin/robot-flow" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == --version ]]; then
  case "${TEST_MODE:-}" in
    version_failure) exit 1 ;;
    version_mismatch) echo 0.1.1 ;;
    *) echo 0.1.24 ;;
  esac
  exit 0
fi
printf '%s\n' "$*" >> "$TEST_ROOT/calls"
dry=false
output=''
scenario="${!#}"
while (($#)); do
  case "$1" in
    --dry-run) dry=true; shift ;;
    --artifacts) output="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if $dry; then [[ "${TEST_MODE:-}" != preflight_failure ]]; exit; fi
if [[ "$scenario" == *warmup.yaml ]]; then
  [[ "${TEST_MODE:-}" != warmup_failure ]]; exit
fi
if [[ "${TEST_MODE:-}" == cancel ]]; then kill -TERM "$PPID"; exit 143; fi
if [[ "${TEST_MODE:-}" == missing_report ]]; then exit 0; fi
mkdir -p "$output/case-1"
status=PASS
limits='[]'
if [[ "$scenario" == *it-04.yaml ]]; then status=PARTIAL; limits='["Busy state"]'; fi
printf '{"status":"%s","dry_run":false,"error":null,"limitations":%s}\n' "$status" "$limits" > "$output/case-1/report.json"
printf '<testsuite tests="1"><testcase name="fixture"/></testsuite>\n' > "$output/case-1/junit.xml"
case "${TEST_MODE:-}" in
  missing_video) ;;
  invalid_video) echo broken > "$output/case-1/video.mp4" ;;
  *) cp "$TEST_ROOT/video.mp4" "$output/case-1/video.mp4" ;;
esac
[[ "${TEST_MODE:-}" != runner_failure ]]
SH
cat > "$root/bin/adb" <<'SH'
#!/bin/sh
printf '%s\n' "$*" >> "$TEST_ROOT/setup-calls"
SH
cat > "$root/bin/xcrun" <<'SH'
#!/bin/sh
echo 'found nothing to terminate' >&2
exit 1
SH
cat > "$root/bin/cygpath" <<'SH'
#!/bin/sh
echo "$2"
SH
cat > "$root/bin/powershell.exe" <<'SH'
#!/bin/sh
echo "$*" >> "$TEST_ROOT/windows-calls"
case "${TEST_MODE:-}:$*" in
  windows_start_failure:*'-Action start'*) exit 1 ;;
  windows_stop_failure:*'-Action stop'*) exit 1 ;;
esac
SH
chmod +x "$root/bin/"*
run_suite() {
  mode="$1"
  env -u ORBIT_PROFILES -u ANDROID_ADB_PATH -u XCRUN_PATH \
    PATH="$root/bin:$PATH" TEST_ROOT="$root" TEST_MODE="$mode" \
    PLATFORM="${2:-android}" DEVICE_ID=fake-device APP_ID=test.app \
    DEVICE_BOT_BIN="$root/bin/device-bot" ROBOT_FLOW_BIN="$root/bin/robot-flow" \
    YAML_ARTIFACT_DIR="$root/results/$mode" \
    bash "$root/app/ci/run_yaml.sh" > "$root/stdout" 2> "$root/stderr"
}
expect_failure() {
  if run_suite "$@"; then echo "Expected $1 to fail" >&2; exit 1; fi
}
run_suite success || { cat "$root/stderr" >&2; exit 1; }
[[ "$(find "$root/results/success" -name report.json | wc -l | tr -d ' ')" == 9 ]]
[[ "$(wc -l < "$root/setup-calls" | tr -d ' ')" == 9 ]]
grep -q 'it-04: PARTIAL' "$root/results/success/QA_REVIEW.md"
grep -q 'Not covered: Busy state' "$root/results/success/QA_REVIEW.md"
[[ "$(wc -l < "$root/calls" | tr -d ' ')" == 10 ]]
if grep -qE 'other.yaml|it-10.yaml.draft' "$root/calls"; then exit 1; fi
# Reusing a successful evidence directory must fail without modifying it.
expect_failure success
for mode in missing_video invalid_video runner_failure missing_report; do
  expect_failure "$mode"
  [[ -f "$root/results/$mode/it-01/ci-error/setup/junit.xml" ]]
done
expect_failure preflight_failure
[[ -f "$root/results/preflight_failure/setup-error/setup/junit.xml" ]]
for mode in version_failure version_mismatch; do
  expect_failure "$mode"
  [[ -f "$root/results/$mode/setup-error/setup/junit.xml" ]]
done
expect_failure warmup_failure ios-sim
[[ -f "$root/results/warmup_failure/setup-error/setup/junit.xml" ]]
expect_failure cancel
[[ -f "$root/results/cancel/setup-error/setup/junit.xml" ]]
run_suite windows windows
[[ "$(grep -c -- '-Action start' "$root/windows-calls")" == 9 ]]
[[ "$(grep -c -- '-Action stop' "$root/windows-calls")" == 9 ]]
for mode in windows_start_failure windows_stop_failure; do
  expect_failure "$mode" windows
  [[ -f "$root/results/$mode/setup-error/setup/junit.xml" ]]
done
# Nested dry-run and warmup reports cannot hide a missing case report.
directory="$root/report-fallback"
mkdir -p "$directory/preflight/case-1" "$directory/warmup/case-1"
touch "$directory/preflight/case-1/junit.xml" "$directory/warmup/case-1/junit.xml"
bash "$app/automation_test/support/ensure_yaml_report.sh" "$directory"
[[ -f "$directory/setup/junit.xml" ]]
mkdir -p "$root/existing-report/case-1"
echo original > "$root/existing-report/case-1/junit.xml"
bash "$app/automation_test/support/ensure_yaml_report.sh" "$root/existing-report"
[[ ! -d "$root/existing-report/setup" ]]
printf 'YAML CI contracts passed: case selection, reports, video errors, setup, cancellation and Windows lifecycle.\n'

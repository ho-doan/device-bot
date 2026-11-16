#!/usr/bin/env bash
# Preserve an error when setup or evidence validation failed outside robot-flow.
set -euo pipefail
directory="${1:?Pass the YAML artifact directory}"
# Only accept a direct case report, never a nested warmup/preflight report.
shopt -s nullglob
reports=("$directory"/case-*/junit.xml "$directory"/setup/junit.xml)
if ((${#reports[@]})); then
  exit 0
fi
mkdir -p "$directory/setup"
cat > "$directory/setup/junit.xml" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="mobile-yaml-ci" tests="1" errors="1"><testcase name="setup-or-evidence"><error message="CI setup or evidence validation failed; inspect job and runner/video logs."/></testcase></testsuite>
XML

#!/usr/bin/env bash
# Check the remaining shell sources; Flutter/Cargo validate their own sources.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
while IFS= read -r -d '' script; do
  bash -n "$script"
done < <(find ci automation_test -type f -name '*.sh' -print0)
printf 'Shell syntax checks passed. Device scenarios were not run.\n'

#!/usr/bin/env bash
# GitLab Windows shell runners may launch jobs in PowerShell. Enter Git Bash
# explicitly for the shared scenario helpers and evidence lifecycle.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mkdir -p artifacts
source ci/ensure_device_bot.sh > artifacts/device-bot-install.log 2>&1
cat artifacts/device-bot-install.log
bash ci/prepare_app.sh 2>&1 | tee artifacts/build.log
export APP_ID="$(cygpath -m "$PWD/build/windows/x64/runner/Debug/flutter_app.exe")"
python3 ci/run_suite.py

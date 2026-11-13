#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/server/flutter_app"
cd "$app"
flutter build web --base-href /qc/
echo "QC web assets: $app/build/web"
echo "device-bot serves them at /qc/ when that directory exists, or set DEVICE_BOT_QC_WEB_DIR."

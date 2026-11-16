#!/usr/bin/env bash
# Build the app from this checkout and install it on the runner's device.
set -euo pipefail
: "${DEVICE_ID:?Set DEVICE_ID}"
: "${PLATFORM:?Set PLATFORM}"
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
unset SDKROOT
flutter pub get
case "$PLATFORM" in
  ios-sim)
    # This runner owns this Simulator for the job. Stop its old cached agent
    # before booting a clean UI/recording session; do not erase app/user data.
    while read -r pid parent command; do
      if [[ "$parent" == 1 && "$command" == *'xcodebuild test-without-building'* &&
            "$command" == *MobileMcpSimulatorRunner* && "$command" == *"id=$DEVICE_ID"* ]]; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
    done < <(ps -ax -o pid=,ppid=,command=)
    state="$(xcrun simctl list devices --json | jq -er --arg id "$DEVICE_ID" \
      '.devices[][] | select(.udid==$id) | .state')"
    if [[ "$state" == Booted ]]; then xcrun simctl shutdown "$DEVICE_ID"; fi
    xcrun simctl boot "$DEVICE_ID"
    xcrun simctl bootstatus "$DEVICE_ID" -b
    flutter build ios --simulator --debug --target lib/main.dart --config-only
    # Xcode 27 lipo rejects Flutter's multi-architecture verify_arch invocation.
    # Build only the runner host architecture for its selected Simulator.
    xcrun xcodebuild -workspace ios/Runner.xcworkspace -scheme Runner \
      -configuration Debug -sdk iphonesimulator \
      -destination "platform=iOS Simulator,id=$DEVICE_ID" \
      -derivedDataPath build/ios-ci \
      "ARCHS=$(uname -m)" ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO build
    xcrun simctl install "$DEVICE_ID" build/ios-ci/Build/Products/Debug-iphonesimulator/Runner.app
    ;;
  android)
    adb -s "$DEVICE_ID" get-state
    flutter build apk --debug --target lib/main.dart
    adb -s "$DEVICE_ID" install -r build/app/outputs/flutter-apk/app-debug.apk
    ;;
  windows)
    flutter build windows --debug --target lib/main.dart
    ;;
  *) printf 'Unsupported PLATFORM: %s\n' "$PLATFORM" >&2; exit 2 ;;
esac

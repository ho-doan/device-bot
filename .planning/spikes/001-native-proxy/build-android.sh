#!/bin/sh
set -eu

source_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
sdk=${ANDROID_SDK_ROOT:?Set ANDROID_SDK_ROOT}
java=${JAVA_HOME:?Set JAVA_HOME}
tools=$sdk/build-tools/${BUILD_TOOLS_VERSION:-36.0.0}
platform=$sdk/platforms/android-35/android.jar
work=$(mktemp -d)
mkdir -p "$work/classes" "$work/dex"
"$java/bin/javac" --release 8 -classpath "$platform" -d "$work/classes" "$source_dir/ProbeActivity.java"
"$tools/d8" --min-api 26 --lib "$platform" --output "$work/dex" "$work/classes/dev/devicebot/proxyspike/ProbeActivity.class"
"$tools/aapt2" link -I "$platform" --manifest "$source_dir/AndroidManifest.xml" -o "$work/unsigned.apk"
(cd "$work/dex" && zip -q "$work/unsigned.apk" classes.dex)
"$tools/zipalign" -f 4 "$work/unsigned.apk" "$work/aligned.apk"
"$java/bin/keytool" -genkeypair -keystore "$work/test.jks" -storepass spike-test-only -keypass spike-test-only -keyalg RSA -dname 'CN=Disposable Proxy Spike' -alias test -validity 2 >&2
JAVA_HOME="$java" "$tools/apksigner" sign --ks "$work/test.jks" --ks-pass pass:spike-test-only --out "$work/Probe.apk" "$work/aligned.apk"
printf '%s\n' "$work/Probe.apk"

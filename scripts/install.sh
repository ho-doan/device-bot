#!/bin/sh

set -eu

VERSION="${DEVICE_BOT_VERSION:-}"
INSTALL_DIR="${DEVICE_BOT_INSTALL_DIR:-$HOME/.local/bin}"
REPOSITORY="${DEVICE_BOT_REPOSITORY:-ho-doan/device-bot}"
BINARY_NAME="device-bot"

for command in curl shasum tar install mkdir mktemp awk sleep cp mv env; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Required command not found: ${command}" >&2
    exit 1
  fi
done

case "$(uname -s)" in
  Darwin)
    ;;
  *)
    echo "This installer supports macOS only." >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  arm64|aarch64)
    TARGET="aarch64-apple-darwin"
    ;;
  x86_64|amd64)
    TARGET="x86_64-apple-darwin"
    ;;
  *)
    echo "Unsupported macOS architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [ -z "$VERSION" ]; then
  release_json="$(curl --fail --silent --show-error --location --retry 3 \
    --proto '=https' --tlsv1.2 \
    "https://api.github.com/repos/${REPOSITORY}/releases/latest")"
  VERSION="$(printf '%s\n' "$release_json" | awk -F'"' '/"tag_name"[[:space:]]*:/ { sub(/^v/, "", $4); print $4; exit }')"
fi

VERSION="${VERSION#v}"
case "$VERSION" in
  ''|*[!A-Za-z0-9._-]*)
    echo "Invalid release version: ${VERSION}" >&2
    exit 1
    ;;
esac
if ! printf '%s\n' "$VERSION" | awk '/^[0-9]+\.[0-9]+\.[0-9]+$/ {ok=1} END {exit !ok}'; then
  echo "Version must be a numeric release such as 0.1.1." >&2
  exit 1
fi

ARCHIVE="${BINARY_NAME}-v${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPOSITORY}/releases/download/v${VERSION}"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/device-bot-install.XXXXXX")"
ARCHIVE_PATH="${TEMP_DIR}/${ARCHIVE}"
MANIFEST_PATH="${TEMP_DIR}/checksums.txt"

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT HUP INT TERM

download_with_retry() {
  download_url="$1"
  download_path="$2"
  download_description="$3"
  download_attempt=1
  while [ "$download_attempt" -le 6 ]; do
    echo "${download_description} (${download_attempt}/6)..."
    if curl --fail --silent --show-error --location --retry 3 --retry-delay 2 \
      --proto '=https' --tlsv1.2 \
      -H 'Cache-Control: no-cache' -H 'Pragma: no-cache' \
      --output "$download_path" "$download_url"
    then
      return 0
    fi
    if [ "$download_attempt" -lt 6 ]; then
      case "$download_attempt" in
        1) download_delay=2 ;;
        2) download_delay=4 ;;
        *) download_delay=8 ;;
      esac
      echo "Release metadata is still propagating; retrying in ${download_delay}s..." >&2
      sleep "$download_delay"
    fi
    download_attempt=$((download_attempt + 1))
  done
  echo "${download_description} failed after 6 attempts." >&2
  return 1
}

echo "Downloading Device Bot ${VERSION} for ${TARGET}..."
download_with_retry "${BASE_URL}/checksums.txt" "$MANIFEST_PATH" "Downloading installer metadata"
download_with_retry "${BASE_URL}/${ARCHIVE}" "$ARCHIVE_PATH" "Downloading Device Bot ${VERSION} for ${TARGET}"

EXPECTED_SHA256="$(awk -v archive="$ARCHIVE" \
  '$2 == archive || $2 == "*" archive { print tolower($1); exit }' "$MANIFEST_PATH")"
case "$EXPECTED_SHA256" in
  ''|*[!0-9a-f]*)
    echo "No verified checksum is available for ${ARCHIVE}." >&2
    exit 1
    ;;
esac
if [ "${#EXPECTED_SHA256}" -ne 64 ]; then
  echo "Invalid checksum metadata for ${ARCHIVE}." >&2
  exit 1
fi

ACTUAL_SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print tolower($1)}')"
if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
  echo "Checksum verification failed." >&2
  echo "Expected: ${EXPECTED_SHA256}" >&2
  echo "Actual:   ${ACTUAL_SHA256}" >&2
  exit 1
fi

tar -xzf "$ARCHIVE_PATH" -C "$TEMP_DIR"
if [ ! -f "${TEMP_DIR}/${BINARY_NAME}" ]; then
  echo "The release archive does not contain ${BINARY_NAME}." >&2
  exit 1
fi

ASSETS="device-bot-assets/${VERSION}"
if [ ! -f "${TEMP_DIR}/${ASSETS}/manifest.json" ]; then
  echo "The release archive does not contain versioned XCTest assets." >&2
  exit 1
fi
# Native CLI suite is part of release archives from 0.1.4 onward.
BINARIES="device-bot"
if printf '%s\n' "$VERSION" | awk -F. '{exit !(($1>0)||($2>1)||($2==1 && $3>=4))}'; then
  BINARIES="device-bot robot-flow robot-report"
fi
for binary in $BINARIES; do
  if [ ! -f "${TEMP_DIR}/${binary}" ]; then
    echo "The release archive does not contain ${binary}." >&2
    exit 1
  fi
  if [ "$("${TEMP_DIR}/${binary}" --version)" != "$VERSION" ]; then
    echo "Release binary version mismatch: ${binary}." >&2
    exit 1
  fi
done
env -u DEVICE_BOT_ASSET_DIR "${TEMP_DIR}/${BINARY_NAME}" --check-install

mkdir -p "$INSTALL_DIR"
mkdir -p "${INSTALL_DIR}/device-bot-assets"
cp -R "${TEMP_DIR}/${ASSETS}" "${INSTALL_DIR}/device-bot-assets/"
if [ -d "${TEMP_DIR}/robot-report-assets/${VERSION}" ]; then
  mkdir -p "${INSTALL_DIR}/robot-report-assets"
  cp -R "${TEMP_DIR}/robot-report-assets/${VERSION}" "${INSTALL_DIR}/robot-report-assets/"
fi
for binary in $BINARIES; do
  install -m 0755 "${TEMP_DIR}/${binary}" "${INSTALL_DIR}/.${binary}-install.$$"
  mv "${INSTALL_DIR}/.${binary}-install.$$" "${INSTALL_DIR}/${binary}"
done
if [ -d "${TEMP_DIR}/qc" ]; then
  rm -rf "${INSTALL_DIR}/qc"
  cp -R "${TEMP_DIR}/qc" "${INSTALL_DIR}/qc"
fi
env -u DEVICE_BOT_ASSET_DIR "${INSTALL_DIR}/${BINARY_NAME}" --check-install

echo "Installed ${BINARIES} ${VERSION} to ${INSTALL_DIR}"
if [ -d "${INSTALL_DIR}/qc" ]; then
  echo "Installed QC to ${INSTALL_DIR}/qc"
fi
case ":${PATH:-}:" in
  *:"${INSTALL_DIR}":*)
    ;;
  *)
    echo "Add it to PATH with: export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
echo "Run: ${BINARY_NAME}"

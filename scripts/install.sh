#!/bin/sh

set -eu

VERSION="${DEVICE_BOT_VERSION:-}"
INSTALL_DIR="${DEVICE_BOT_INSTALL_DIR:-$HOME/.local/bin}"
REPOSITORY="${DEVICE_BOT_REPOSITORY:-ho-doan/device-bot}"
BINARY_NAME="device-bot"

for command in curl shasum tar install mkdir mktemp awk sleep; do
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

case "$VERSION" in
  ''|*[!A-Za-z0-9._-]*)
    echo "Invalid release version: ${VERSION}" >&2
    exit 1
    ;;
esac

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

mkdir -p "$INSTALL_DIR"
install -m 0755 "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"

echo "Installed ${BINARY_NAME} ${VERSION} to ${INSTALL_DIR}/${BINARY_NAME}"
case ":${PATH:-}:" in
  *:"${INSTALL_DIR}":*)
    ;;
  *)
    echo "Add it to PATH with: export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
echo "Run: ${BINARY_NAME}"

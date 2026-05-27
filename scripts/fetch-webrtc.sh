#!/usr/bin/env bash
#
# Fetch the pinned godotengine/webrtc-native GDExtension into
# client/addons/webrtc/. The native binaries are NOT committed (see
# client/.gitignore) — CI and developers run this once before opening or
# exporting the Godot client. Idempotent: re-run is a no-op once installed.
#
# Without this extension, Godot's built-in WebRtcPeerConnection is
# interface-only (WebRtcPeerConnection.Create() returns null), so no peer
# connection can be made. See docs/planning/multiplayer-client-stages.md.
set -euo pipefail

VERSION="1.1.0-stable"          # webrtc-native release tag (Godot 4.1+; compat 4.6.3)
ASSET="godot-extension-webrtc.zip"
URL="https://github.com/godotengine/webrtc-native/releases/download/${VERSION}/${ASSET}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ADDONS_DIR="${SCRIPT_DIR}/../client/addons"
DEST="${ADDONS_DIR}/webrtc"
MARKER="${DEST}/webrtc.gdextension"

if [ -f "$MARKER" ]; then
  echo "webrtc-native already present at ${DEST} — skipping (delete the dir to re-fetch)."
  exit 0
fi

echo "Fetching webrtc-native ${VERSION}…"
mkdir -p "$ADDONS_DIR"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL -o "${TMP}/webrtc.zip" "$URL"

# The archive's top-level folder is `webrtc/`, so extracting into ADDONS_DIR
# produces client/addons/webrtc/. The zip has no symlinks, so either tool is
# safe; prefer unzip when available, fall back to Python's zipfile.
if command -v unzip >/dev/null 2>&1; then
  unzip -q -o "${TMP}/webrtc.zip" -d "$ADDONS_DIR"
else
  python3 -m zipfile -e "${TMP}/webrtc.zip" "$ADDONS_DIR"
fi

if [ ! -f "$MARKER" ]; then
  echo "ERROR: expected ${MARKER} after extraction — did the archive layout change?" >&2
  exit 1
fi

echo "webrtc-native ${VERSION} installed at ${DEST}"

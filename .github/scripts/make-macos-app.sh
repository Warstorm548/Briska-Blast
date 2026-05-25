#!/usr/bin/env bash
#
# Assemble an ad-hoc-signed macOS .app bundle + .dmg for the BriskaBlast launcher.
#
# Usage: make-macos-app.sh <binary> <version> <icon-png> <out-dir>
#   <binary>    path to the built briskablast-launcher Mach-O (arm64)
#   <version>   semver string for CFBundleShortVersionString / .dmg name
#   <icon-png>  source PNG for the .icns (icon.png; ideally >=512x512)
#   <out-dir>   directory to write "<APP_NAME>.app" and the .dmg into
#
# Ad-hoc signing (`codesign --sign -`) is tester-grade: the app runs locally past
# Gatekeeper for a known tester (right-click -> Open the first time, or
# `xattr -d com.apple.quarantine`). It is NOT a Developer-ID signature and is NOT
# notarized, so it is not for public distribution. macOS-only — relies on
# sips / iconutil / codesign / hdiutil.
set -euo pipefail

BINARY="$1"
VERSION="$2"
ICON_PNG="$3"
OUT_DIR="$4"

APP_NAME="BriskaBlast Launcher"
BUNDLE_ID="com.phoenixwired.briskablast.launcher"
EXEC_NAME="briskablast-launcher"

APP="${OUT_DIR}/${APP_NAME}.app"
CONTENTS="${APP}/Contents"

# Temp dirs (assigned below) are removed on exit — success or failure. Guarded
# with :- so the trap is safe under `set -u` before they're assigned.
ICONSET_TMP=""
STAGING_TMP=""
trap 'rm -rf "${ICONSET_TMP:-}" "${STAGING_TMP:-}"' EXIT

rm -rf "$APP"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"

# --- binary ---
cp "$BINARY" "${CONTENTS}/MacOS/${EXEC_NAME}"
chmod +x "${CONTENTS}/MacOS/${EXEC_NAME}"

# --- icon: PNG -> .icns via a generated .iconset ---
ICONSET_TMP="$(mktemp -d)"
ICONSET="${ICONSET_TMP}/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ICON_PNG" \
    --out "${ICONSET}/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$ICON_PNG" \
    --out "${ICONSET}/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "${CONTENTS}/Resources/AppIcon.icns"

# --- Info.plist ---
cat > "${CONTENTS}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleExecutable</key><string>${EXEC_NAME}</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# --- ad-hoc sign + verify ---
codesign --force --deep --sign - "$APP"
codesign --verify --strict --verbose=2 "$APP"

# --- .dmg (drag-to-Applications layout) ---
DMG="${OUT_DIR}/briskablast-launcher-${VERSION}-aarch64-apple-darwin.dmg"
STAGING_TMP="$(mktemp -d)"
STAGING="${STAGING_TMP}/dmg"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
rm -f "$DMG"
# `hdiutil create` intermittently fails with "Resource busy" on CI when the
# staging volume is still held by another process (e.g. Spotlight indexing).
# Flush pending writes and retry a few times before giving up.
sync
attempt=0
max_attempts=3
until hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG"; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge "$max_attempts" ]; then
    echo "hdiutil create failed after ${max_attempts} attempts" >&2
    exit 1
  fi
  echo "hdiutil create failed (attempt ${attempt}/${max_attempts}); retrying in 5s..." >&2
  rm -f "$DMG"
  sleep 5
done

echo "Built: $APP"
echo "Built: $DMG"

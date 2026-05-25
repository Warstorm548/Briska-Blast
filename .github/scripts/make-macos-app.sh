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

rm -rf "$APP"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"

# --- binary ---
cp "$BINARY" "${CONTENTS}/MacOS/${EXEC_NAME}"
chmod +x "${CONTENTS}/MacOS/${EXEC_NAME}"

# --- icon: PNG -> .icns via a generated .iconset ---
ICONSET="$(mktemp -d)/AppIcon.iconset"
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
STAGING="$(mktemp -d)/dmg"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG"

echo "Built: $APP"
echo "Built: $DMG"

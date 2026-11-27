#!/usr/bin/env bash
# Build StageCraft.app (Apple Silicon) and wrap it in a drag-to-Applications disk image.
#
#   scripts/macos-dmg.sh [version]      → dist/StageCraft-<version>-macos-arm64.dmg
#
# The app is ad-hoc signed only (no Developer ID, no notarization): the first start needs
# right-click → Open, or: xattr -dr com.apple.quarantine /Applications/StageCraft.app
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
VERSION="${VERSION#v}"
APP_NAME="StageCraft"
BUNDLE_ID="events.rueger.stagecraft"
TARGET=aarch64-apple-darwin
DIST=dist
APP="$DIST/$APP_NAME.app"
DMG="$DIST/StageCraft-$VERSION-macos-arm64.dmg"

rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --release --locked --target "$TARGET" -p stagecraft-app --bin stagecraft

rm -rf "$APP" "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$TARGET/release/stagecraft" "$APP/Contents/MacOS/stagecraft"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>stagecraft</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"

STAGE="$DIST/dmg"
rm -rf "$STAGE" && mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
echo "$DMG"

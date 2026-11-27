#!/usr/bin/env bash
# Build StageCraft.app (Apple Silicon) and wrap it in a drag-to-Applications disk image.
#
#   scripts/macos-dmg.sh [version]      → dist/StageCraft-<version>-macos-arm64.dmg
#
# The app is ad-hoc signed only (no Developer ID, no notarization): the first start needs
# right-click → Open, or: xattr -dr com.apple.quarantine /Applications/StageCraft.app
set -euo pipefail

# The sources to build: STAGECRAFT_ROOT, else the checkout this script is in.
cd "${STAGECRAFT_ROOT:-$(dirname "$0")/..}"
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

# Icon: assets/app-icon/stagecraft-1024.png (rendered from stagecraft.svg) as an .icns.
ICONSET="$DIST/AppIcon.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
SRC=assets/app-icon/stagecraft-1024.png
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$SRC" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$SRC" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

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
  <key>CFBundleIconFile</key><string>AppIcon</string>
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
# An explicit size and file system: left to itself, hdiutil on GitHub's macOS runners
# misjudges the image and fails with "No space left on device". Retried, since it can
# also fail with "Resource busy" right after the copy.
SIZE_MB=$(( $(du -sm "$STAGE" | cut -f1) * 13 / 10 + 20 ))
for attempt in 1 2 3; do
  if hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -fs HFS+ -size "${SIZE_MB}m" -ov -format UDZO "$DMG" >/dev/null; then
    break
  fi
  [ "$attempt" = 3 ] && exit 1
  sleep 5
done
rm -rf "$STAGE"
echo "$DMG"

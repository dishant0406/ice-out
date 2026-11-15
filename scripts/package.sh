#!/bin/sh
# Builds dist/Iceout.app and the installer dist/Iceout.dmg.
# Needs: Rust, the Xcode command line tools, Google Chrome (only to draw the installer picture),
# and network the first time (to fetch dmgbuild, a build tool that only writes the installer
# window's layout; it is not part of the app).
set -e
cd "$(dirname "$0")/.."

OUT=dist
VERSION=0.1.0
rm -rf "$OUT"
mkdir -p "$OUT"

echo "Building..."
cargo build --release

echo "Making the app..."
APP="$OUT/Iceout.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/iceout "$APP/Contents/MacOS/iceout"

# The icon: draw it once at 1024, then scale it for every size macOS asks for.
swift scripts/make-icon.swift "$OUT/icon-1024.png"
ICONSET="$OUT/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size "$OUT/icon-1024.png" --out "$ICONSET/icon_${size}x${size}.png" > /dev/null
    sips -z $((size * 2)) $((size * 2)) "$OUT/icon-1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Ice Out</string>
    <key>CFBundleDisplayName</key><string>Ice Out</string>
    <key>CFBundleIdentifier</key><string>dev.iceout.app</string>
    <key>CFBundleExecutable</key><string>iceout</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# An ad-hoc signature is enough to run on this Mac. Sending the app to other Macs needs a
# Developer ID signature and notarization.
codesign --force --sign - "$APP"

echo "Making the installer..."
./scripts/render-dmg-background.sh "$OUT/background.tiff"
cp "$APP/Contents/Resources/AppIcon.icns" "$OUT/volume.icns"
python3 -m venv "$OUT/.venv"
"$OUT/.venv/bin/pip" install -q dmgbuild
"$OUT/.venv/bin/dmgbuild" -s scripts/dmg-settings.py \
    -D app="$APP" -D background="$OUT/background.tiff" -D icon="$OUT/volume.icns" Iceout "$OUT/Iceout.dmg"

echo "Done: $OUT/Iceout.dmg"

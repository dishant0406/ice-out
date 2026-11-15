#!/bin/sh
# Renders assets/dmg/background.html into the installer's background picture: one TIFF that holds
# a 660 x 400 picture and a sharp 1320 x 800 one, which Finder picks on a Retina screen.
# Needs Google Chrome (it is only used to draw the picture, headless).
# Usage: scripts/render-dmg-background.sh dist/background.tiff
set -e
cd "$(dirname "$0")/.."
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for scale in 1 2; do
    "$CHROME" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$WORK/profile" \
        --force-device-scale-factor=$scale --window-size=660,400 \
        --screenshot="$WORK/background$scale.png" "file://$PWD/assets/dmg/background.html" > /dev/null 2>&1
done
mv "$WORK/background1.png" "$WORK/background.png"
mv "$WORK/background2.png" "$WORK/background@2x.png"
tiffutil -cathidpicheck "$WORK/background.png" "$WORK/background@2x.png" -out "$1" > /dev/null
cp "$WORK/background@2x.png" "${1%.tiff}.preview.png"

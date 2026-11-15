#!/bin/sh
# Builds tools/axprobe/AXProbe.app (ad-hoc signed). Run: tools/axprobe/build.sh
# Run it: open -n -W tools/axprobe/AXProbe.app --args "$PWD/out/ax.txt"
set -e
cd "$(dirname "$0")"
rm -rf AXProbe.app
mkdir -p AXProbe.app/Contents/MacOS
swiftc main.swift -o AXProbe.app/Contents/MacOS/AXProbe
cp Info.plist AXProbe.app/Contents/Info.plist
codesign --force --sign - AXProbe.app

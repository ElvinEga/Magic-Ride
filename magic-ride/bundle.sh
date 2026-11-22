#!/usr/bin/env bash
# Assemble macOS plugin bundles from the released cdylibs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$ROOT/bundles}"
mkdir -p "$OUT"

for fmt in vst3 clap; do
  lib="$ROOT/target/release/libmagic_ride_${fmt}.dylib"
  [ -f "$lib" ] || { echo "missing $lib (run cargo build --release first)"; exit 1; }
  if [ "$fmt" = vst3 ]; then
    bundle="$OUT/magic.RIDE.vst3"
    mkdir -p "$bundle/Contents/MacOS"
    cp "$lib" "$bundle/Contents/MacOS/magic.RIDE"
    cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>magic.RIDE</string>
  <key>CFBundleIdentifier</key><string>com.mbmaudio.magicride.re.vst3</string>
  <key>CFBundleExecutable</key><string>magic.RIDE</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleVersion</key><string>1.3.1</string>
</dict></plist>
PLIST
  else
    bundle="$OUT/magic.RIDE.clap"
    mkdir -p "$bundle/Contents/MacOS"
    cp "$lib" "$bundle/Contents/MacOS/magic.RIDE"
    cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>magic.RIDE</string>
  <key>CFBundleIdentifier</key><string>com.mbmaudio.magicride.re.clap</string>
  <key>CFBundleExecutable</key><string>magic.RIDE</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleVersion</key><string>1.3.1</string>
</dict></plist>
PLIST
  fi
  echo "PkgInfo" > "$bundle/Contents/PkgInfo"
  echo "built $bundle"
done

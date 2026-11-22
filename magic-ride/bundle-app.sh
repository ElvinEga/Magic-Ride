#!/usr/bin/env bash
# Build a double-clickable macOS .app standalone test host.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
APP="$ROOT/magic.RIDE Standalone.app"
BIN="$ROOT/target/release/magic-ride-app"

if [ ! -f "$BIN" ]; then
  echo "Building release binary..."
  cargo build --release -p magic-ride-app --manifest-path "$ROOT/Cargo.toml"
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/magic-ride-app"
chmod +x "$APP/Contents/MacOS/magic-ride-app"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>magic.RIDE Standalone</string>
  <key>CFBundleDisplayName</key><string>magic.RIDE Standalone</string>
  <key>CFBundleIdentifier</key><string>com.mbmaudio.magicride.re.standalone</string>
  <key>CFBundleExecutable</key><string>magic-ride-app</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.3.1</string>
  <key>CFBundleVersion</key><string>1.3.1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>magic.RIDE Standalone listens to the default input device so you can test the reconstructed gain-riding DSP in real time.</string>
</dict></plist>
PLIST

echo "APPL????" > "$APP/Contents/PkgInfo"
echo "Created: $APP"
echo "Open it with: open '$APP'"

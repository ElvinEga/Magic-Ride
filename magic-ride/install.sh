#!/usr/bin/env bash
# Build release VST3 and CLAP with egui GUI and install into ~/Library/Audio/Plug-Ins/
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"

echo "Building and bundling release VST3 and CLAP via xtask..."
cd "$ROOT"
cargo run --package magic-ride-xtask -- bundle magic-ride --release

echo "Installing into ~/Library/Audio/Plug-Ins/..."
mkdir -p "$HOME/Library/Audio/Plug-Ins/VST3" "$HOME/Library/Audio/Plug-Ins/CLAP"
rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/magic.RIDE.vst3" "$HOME/Library/Audio/Plug-Ins/CLAP/magic.RIDE.clap"
cp -R "$ROOT/target/bundled/magic.RIDE.vst3" "$HOME/Library/Audio/Plug-Ins/VST3/"
cp -R "$ROOT/target/bundled/magic.RIDE.clap" "$HOME/Library/Audio/Plug-Ins/CLAP/"

if command -v codesign >/dev/null 2>&1; then
  echo "Ad-hoc code signing installed plugins..."
  codesign -s - --force "$HOME/Library/Audio/Plug-Ins/VST3/magic.RIDE.vst3"
  codesign -s - --force "$HOME/Library/Audio/Plug-Ins/CLAP/magic.RIDE.clap"
fi

if command -v xattr >/dev/null 2>&1; then
  xattr -cr "$HOME/Library/Audio/Plug-Ins/VST3/magic.RIDE.vst3"
  xattr -cr "$HOME/Library/Audio/Plug-Ins/CLAP/magic.RIDE.clap"
fi

echo "Installed successfully:"
echo "  - $HOME/Library/Audio/Plug-Ins/VST3/magic.RIDE.vst3"
echo "  - $HOME/Library/Audio/Plug-Ins/CLAP/magic.RIDE.clap"

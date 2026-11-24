#!/usr/bin/env bash
# Packages the rawpeg binary(ies) into a portable macOS .app bundle and zips it.
#
# Usage:
#   scripts/package_macos.sh <arm64-binary> [<x86_64-binary>]
#
#   - With two binaries, they are merged into a universal (fat) binary.
#   - With one binary, it is used as-is (e.g. a local arm64 build).
#
# Output: rawpeg-macos-universal.zip  (rawpeg.app/...)
set -euo pipefail

ARM_BIN="${1:?usage: package_macos.sh <arm64-binary> [<x86_64-binary>]}"
X64_BIN="${2:-}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGING="$ROOT/target/macos-package"
APP="$STAGING/rawpeg.app"
BIN_DIR="$APP/Contents/MacOS"
RES_DIR="$APP/Contents/Resources"
VERSION="${RAW_PEG_VERSION:-$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -n1)}"

if [ -z "$VERSION" ]; then
  echo "error: cannot determine version from Cargo.toml" >&2
  exit 1
fi

rm -rf "$STAGING"
mkdir -p "$BIN_DIR" "$RES_DIR"
cp "$ROOT/.github/macos/Info.plist" "$APP/Contents/Info.plist"
cp "$ROOT/assets/rawpeg.icns" "$RES_DIR/rawpeg.icns"
plutil -replace CFBundleVersion -string "$VERSION" "$APP/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$APP/Contents/Info.plist"

if [ -n "$X64_BIN" ]; then
  echo "Merging into universal binary: $ARM_BIN + $X64_BIN"
  lipo -create "$ARM_BIN" "$X64_BIN" -output "$BIN_DIR/rawpeg"
else
  echo "Using single binary: $ARM_BIN"
  cp "$ARM_BIN" "$BIN_DIR/rawpeg"
fi

chmod +x "$BIN_DIR/rawpeg"
plutil -lint "$APP/Contents/Info.plist"

echo "Packaging $ROOT/rawpeg-macos-universal.zip"
rm -f "$ROOT/rawpeg-macos-universal.zip"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ROOT/rawpeg-macos-universal.zip"
echo "Done: rawpeg-macos-universal.zip"

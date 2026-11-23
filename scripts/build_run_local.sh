#!/usr/bin/env bash
# Builds the release binary, packages a portable .app bundle and launches it.
#
# Usage:
#   ./scripts/build_run_local.sh            # build + package + run (GUI)
#   ./scripts/build_run_local.sh --no-run   # build + package only
set -euo pipefail

NO_RUN=false
[ "${1:-}" = "--no-run" ] && NO_RUN=true

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/rawpeg"
APP="$ROOT/target/macos-package/rawpeg.app"

echo "==> 1/3 cargo build --release"
cargo build --release

echo "==> 2/3 packaging .app bundle (portable, no install needed)"
"$ROOT/scripts/package_macos.sh" "$BIN"

if [ "$NO_RUN" = true ]; then
    echo "Done (no-run): $APP"
    echo "Launch anytime with: open $APP"
    exit 0
fi

echo "==> 3/3 launching $APP"
open "$APP"
echo "Done."

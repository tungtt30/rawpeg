#!/usr/bin/env bash
# Downloads sample RAW files from raw.pixls.us (CC0 licensed) for manual
# testing. Pass a destination directory as the first argument (default: ./samples).
#
# Usage:
#   scripts/fetch_samples.sh [dest-dir]
set -euo pipefail

DEST="${1:-samples}"
mkdir -p "$DEST"

fetch() {
  local url="$1"
  local name="$2"
  if [ -f "$DEST/$name" ]; then
    echo "skip   $name (already present)"
  else
    echo "fetch  $name"
    curl -sSL -o "$DEST/$name" "$url"
    echo "saved  $DEST/$name"
  fi
}

fetch "https://raw.pixls.us/data/Canon/EOS%206D/EOS_6D_RAW.CR2" "canon_6d.cr2"
fetch "https://raw.pixls.us/data/Nikon/D600/DSC_3294.NEF"       "nikon_d600.nef"

echo
echo "Done. Run the real-sample test with:"
echo "  RAW_SAMPLE=$DEST/canon_6d.cr2 cargo test --test decoder_tests decode_real_sample -- --ignored"

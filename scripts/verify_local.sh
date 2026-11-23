#!/usr/bin/env bash
# Local verification gate (see AGENTS.md). Run after every change:
#   ./scripts/verify_local.sh
# Exit codes: 0 = all green, 1 = any step failed.
set -u

fail() {
    echo "FAIL: $1"
    exit 1
}

step() {
    echo ""
    echo "==> $1"
}

step "Branch"
git branch --show-current

step "1/4 cargo check"
cargo check || fail "cargo check"

step "2/4 cargo test"
cargo test || fail "cargo test"

step "3/4 cargo clippy --all-targets (0 warnings required)"
cargo clippy --all-targets -- -D warnings || fail "cargo clippy"

step "4/4 cargo build (debug)"
cargo build || fail "cargo build"

echo ""
echo "Local verification gate: ALL GREEN"

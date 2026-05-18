#!/usr/bin/env bash
set -euo pipefail

FEATURES="${FEATURES:-semantic,semantic-ts,semantic-python,semantic-bash,mcp,loop,git-worktree,plugin}"

echo "==> Building dirge with features: $FEATURES"
cargo build --features "$FEATURES" --release

echo "==> Binary: target/release/dirge"
ls -lh target/release/dirge

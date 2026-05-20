#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:---generated}"
TARGET_DIR="$ROOT_DIR/target"

cd "$ROOT_DIR"

if [[ "$MODE" != "--generated" && "$MODE" != "--all" ]]; then
  echo "Usage: $0 [--generated|--all]" >&2
  exit 1
fi

pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true

if [[ -d "$TARGET_DIR" ]]; then
  rm -rf \
    "$TARGET_DIR/render-regression" \
    "$TARGET_DIR/space-test" \
    "$TARGET_DIR/space-test-smoke" \
    "$TARGET_DIR/space-test-live.out" \
    "$TARGET_DIR/space-test-live.pid" \
    "$TARGET_DIR/space-test-smoke.out" \
    "$TARGET_DIR/vtube-studio-rs.pid"
fi

if [[ "$MODE" == "--all" ]]; then
  cargo clean
fi

echo "Cleaned target artifacts ($MODE)."

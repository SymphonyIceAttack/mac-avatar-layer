#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
WAIT_SECONDS="${WAIT_SECONDS:-25}"
REPORT_TIMEOUT_SECONDS="${REPORT_TIMEOUT_SECONDS:-30}"

export WAIT_SECONDS
export REPORT_TIMEOUT_SECONDS

cleanup() {
  pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
  rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

run_step() {
  local name="$1"
  shift

  echo
  echo "==> $name"
  cleanup
  VTUBE_RS_SKIP_TARGET_CLEAN=1 VTUBE_RS_SKIP_REPORT=1 "$@"
}

mkdir -p "$OUTPUT_DIR"
"$ROOT_DIR/scripts/clean-target.sh" --generated
mkdir -p "$OUTPUT_DIR"

run_step "Risk model sweep" "$ROOT_DIR/scripts/capture-risk-models.sh"
run_step "Mao mask matrix" "$ROOT_DIR/scripts/capture-mask-matrix.sh"
run_step "Ren offscreen matrix" "$ROOT_DIR/scripts/capture-offscreen-matrix.sh"

if [[ -f "$ROOT_DIR/public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json" ]]; then
  run_step "Rice optional stress matrix" "$ROOT_DIR/scripts/capture-rice-stress.sh"
else
  echo
  echo "==> Rice optional stress matrix"
  echo "Skipping missing optional Rice sample model."
fi

run_step "Texture quality matrix" "$ROOT_DIR/scripts/capture-quality-matrix.sh"
cleanup

PROBE_MODELS=(
  "public/model/0.model3.json"
  "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json"
  "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json"
)
if [[ -f "$ROOT_DIR/public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json" ]]; then
  PROBE_MODELS+=("public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json")
fi

echo
echo "==> Combined model risk probe"
"$ROOT_DIR/scripts/probe-risk-models.sh" "${PROBE_MODELS[@]}"

echo
"$ROOT_DIR/scripts/render-regression-report-safe.sh"

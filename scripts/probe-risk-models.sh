#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
PROBE_PATH="${PROBE_PATH:-$OUTPUT_DIR/probe.txt}"
SDK_ROOT="${LIVE2D_CUBISM_SDK_NATIVE_DIR:-$ROOT_DIR/public/CubismSdkForNative}"

if [[ -z "${CUBISM_CORE_INCLUDE_DIR:-}" ]]; then
  export CUBISM_CORE_INCLUDE_DIR="$SDK_ROOT/Core/include"
fi

if [[ -z "${CUBISM_CORE_LIB_DIR:-}" ]]; then
  arch_name="$(uname -m)"
  case "$arch_name" in
    arm64) export CUBISM_CORE_LIB_DIR="$SDK_ROOT/Core/lib/macos/arm64" ;;
    x86_64) export CUBISM_CORE_LIB_DIR="$SDK_ROOT/Core/lib/macos/x86_64" ;;
    *)
      echo "Unsupported macOS architecture: $arch_name" >&2
      exit 1
      ;;
  esac
fi

if [[ ! -f "$CUBISM_CORE_INCLUDE_DIR/Live2DCubismCore.h" ]]; then
  echo "Missing Live2DCubismCore.h at: $CUBISM_CORE_INCLUDE_DIR" >&2
  exit 1
fi

if [[ ! -f "$CUBISM_CORE_LIB_DIR/libLive2DCubismCore.a" ]]; then
  echo "Missing libLive2DCubismCore.a at: $CUBISM_CORE_LIB_DIR" >&2
  exit 1
fi

if [[ "$#" -gt 0 ]]; then
  ROOTS=("$@")
else
  ROOTS=(
    "public/model"
    "public/CubismSdkForNative/Samples/Resources/Mao"
    "public/CubismSdkForNative/Samples/Resources/Ren"
  )
fi

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"

{
  echo "# vtube-studio-rs Model Risk Probe"
  echo
  echo "Generated: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "Roots: ${ROOTS[*]}"
  echo
  cargo run --features metal-renderer -- --probe-models "${ROOTS[@]}"
} >"$PROBE_PATH"

echo "$PROBE_PATH"

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${1:-public/model/0.model3.json}"
SDK_ROOT="${LIVE2D_CUBISM_SDK_NATIVE_DIR:-$ROOT_DIR/public/CubismSdkForNative}"

if [[ "${RUN_METAL_KILL_OLD:-1}" != "0" ]]; then
  pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
  rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"
fi

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
  echo "Set LIVE2D_CUBISM_SDK_NATIVE_DIR, CUBISM_CORE_INCLUDE_DIR, or install the SDK under public/CubismSdkForNative." >&2
  exit 1
fi

if [[ ! -f "$CUBISM_CORE_LIB_DIR/libLive2DCubismCore.a" ]]; then
  echo "Missing libLive2DCubismCore.a at: $CUBISM_CORE_LIB_DIR" >&2
  echo "Set CUBISM_CORE_LIB_DIR or install the SDK under public/CubismSdkForNative." >&2
  exit 1
fi

cd "$ROOT_DIR"
exec cargo run --features metal-renderer -- "$MODEL_PATH"

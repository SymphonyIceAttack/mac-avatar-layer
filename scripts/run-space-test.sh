#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${1:-public/model/0.model3.json}"
SDK_ROOT="${LIVE2D_CUBISM_SDK_NATIVE_DIR:-$ROOT_DIR/public/CubismSdkForNative}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/space-test}"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
LOG_PATH="$OUTPUT_DIR/space-test-$TIMESTAMP.log"

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
mkdir -p "$OUTPUT_DIR"
pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"

echo "Starting vtube-studio-rs Space/display reliability run."
echo "Model: $MODEL_PATH"
echo "Log: $LOG_PATH"
echo
echo "Manual steps:"
echo "  1. Wait for the avatar window."
echo "  2. Switch macOS Spaces several times."
echo "  3. Test beside a full-screen app."
echo "  4. Optionally test display sleep/wake."
echo "  5. Press Ctrl-C here to stop and print the event summary."
echo

touch "$LOG_PATH"
cargo run --features metal-renderer -- "$MODEL_PATH" >"$LOG_PATH" 2>&1 &
APP_PID=$!
tail -f "$LOG_PATH" &
TAIL_PID=$!
CLEANED_UP=0

cleanup() {
  set +e
  if [[ "$CLEANED_UP" == "1" ]]; then
    return
  fi
  CLEANED_UP=1
  kill "$TAIL_PID" 2>/dev/null || true
  kill "$APP_PID" 2>/dev/null || true
  pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
  wait "$TAIL_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
  rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"
  echo
  echo "Space/display event summary:"
  if [[ -f "$LOG_PATH" ]]; then
    printf "  %-34s %s\n" "instance_guard_acquired" "$(grep -c 'renderer_event=instance_guard_acquired' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "app_nap_guard_started" "$(grep -c 'renderer_event=app_nap_guard_started' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "window_configured" "$(grep -c 'renderer_event=window_configured' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "long_frame_gap" "$(grep -c 'renderer_event=long_frame_gap' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "next_drawable_unavailable" "$(grep -c 'renderer_event=next_drawable_unavailable' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "next_drawable_recovered" "$(grep -c 'renderer_event=next_drawable_recovered' "$LOG_PATH" || true)"
    printf "  %-34s %s\n" "drawable_size_changed" "$(grep -c 'renderer_event=drawable_size_changed' "$LOG_PATH" || true)"
    echo
    echo "Recent renderer events:"
    grep 'renderer_event=' "$LOG_PATH" | tail -20 || true
    echo
    echo "Full log: $LOG_PATH"
  else
    echo "  Log file was not created."
  fi
}

trap cleanup EXIT INT TERM
wait "$APP_PID"

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${1:-public/model/0.model3.json}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
SDK_ROOT="${LIVE2D_CUBISM_SDK_NATIVE_DIR:-$ROOT_DIR/public/CubismSdkForNative}"
WAIT_SECONDS="${WAIT_SECONDS:-12}"
POST_WINDOW_WAIT_SECONDS="${POST_WINDOW_WAIT_SECONDS:-1.25}"

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

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"
pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"

cargo run --features metal-renderer -- "$MODEL_PATH" >"$OUTPUT_DIR/capture.log" 2>&1 &
APP_PID=$!
cleanup() {
  kill "$APP_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
  rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"
}
trap cleanup EXIT

WINDOW_ID=""
deadline=$((SECONDS + WAIT_SECONDS))
while [[ -z "$WINDOW_ID" && "$SECONDS" -lt "$deadline" ]]; do
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "vtube-studio-rs exited before a window appeared. Last log lines:" >&2
    tail -40 "$OUTPUT_DIR/capture.log" >&2 || true
    exit 1
  fi

  WINDOW_ID="$(
    swift -e 'import CoreGraphics
let opts = CGWindowListOption(arrayLiteral: .optionOnScreenOnly)
if let list = CGWindowListCopyWindowInfo(opts, CGWindowID(0)) as? [[String: Any]] {
    for window in list {
        let owner = window[kCGWindowOwnerName as String] as? String ?? ""
        if owner == "vtube-studio-rs" {
            print(window[kCGWindowNumber as String] ?? "")
            break
        }
    }
}'
  )"
  if [[ -z "$WINDOW_ID" ]]; then
    sleep 0.25
  fi
done

if [[ -z "$WINDOW_ID" ]]; then
  echo "Could not find vtube-studio-rs window. Last log lines:" >&2
  tail -40 "$OUTPUT_DIR/capture.log" >&2 || true
  exit 1
fi

sleep "$POST_WINDOW_WAIT_SECONDS"

MODEL_NAME="$(basename "$MODEL_PATH" .model3.json)"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
OUTPUT_PATH="$OUTPUT_DIR/${MODEL_NAME}-${TIMESTAMP}.png"
CAPTURE_ATTEMPTS="${CAPTURE_ATTEMPTS:-5}"
for attempt in $(seq 1 "$CAPTURE_ATTEMPTS"); do
  if screencapture -x -l "$WINDOW_ID" "$OUTPUT_PATH" 2>"$OUTPUT_DIR/capture-screencapture.err"; then
    echo "$OUTPUT_PATH"
    exit 0
  fi
  if [[ "$attempt" == "$CAPTURE_ATTEMPTS" ]]; then
    echo "Could not capture vtube-studio-rs window after $CAPTURE_ATTEMPTS attempts. Last screencapture error:" >&2
    cat "$OUTPUT_DIR/capture-screencapture.err" >&2 || true
    echo "Last app log lines:" >&2
    tail -40 "$OUTPUT_DIR/capture.log" >&2 || true
    exit 1
  fi
  sleep 0.5
done
echo "$OUTPUT_PATH"

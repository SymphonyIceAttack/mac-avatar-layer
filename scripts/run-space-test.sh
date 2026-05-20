#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${1:-public/model/0.model3.json}"
SDK_ROOT="${LIVE2D_CUBISM_SDK_NATIVE_DIR:-$ROOT_DIR/public/CubismSdkForNative}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/space-test}"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
LOG_PATH="$OUTPUT_DIR/space-test-$TIMESTAMP.log"
REPORT_PATH="$OUTPUT_DIR/space-test-$TIMESTAMP.md"

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
if [[ "${VTUBE_RS_SKIP_TARGET_CLEAN:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/clean-target.sh" --generated
  mkdir -p "$OUTPUT_DIR"
fi
pkill -f "$ROOT_DIR/target/debug/vtube-studio-rs" 2>/dev/null || true
rm -f "$ROOT_DIR/target/vtube-studio-rs.pid"

echo "Starting vtube-studio-rs Space/display reliability run."
echo "Model: $MODEL_PATH"
echo "Log: $LOG_PATH"
echo "Report: $REPORT_PATH"
echo
echo "Checklist:"
echo "  [ ] Wait for the avatar window and confirm Frames keep increasing."
echo "  [ ] Switch between macOS Spaces several times."
echo "  [ ] Place the avatar beside a full-screen app and confirm it remains visible."
echo "  [ ] Optionally test display sleep/wake and confirm the avatar recovers."
echo "  [ ] Confirm reruns do not leave duplicate avatar windows."
echo "  [ ] Press Ctrl-C here to stop, print the summary, and write the report."
echo

touch "$LOG_PATH"
cargo run --features metal-renderer -- "$MODEL_PATH" >"$LOG_PATH" 2>&1 &
APP_PID=$!
tail -f "$LOG_PATH" &
TAIL_PID=$!
CLEANED_UP=0

event_count() {
  grep -c "renderer_event=$1" "$LOG_PATH" 2>/dev/null || true
}

status_line() {
  local label="$1"
  local status="$2"
  local detail="$3"
  printf "| %s | %s | %s |\n" "$label" "$status" "$detail"
}

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
    instance_guard_acquired="$(event_count instance_guard_acquired)"
    app_nap_guard_started="$(event_count app_nap_guard_started)"
    window_configured="$(event_count window_configured)"
    app_active_changed="$(event_count app_active_changed)"
    window_visible_changed="$(event_count window_visible_changed)"
    window_occlusion_changed="$(event_count window_occlusion_changed)"
    long_frame_gap="$(event_count long_frame_gap)"
    display_wake_inferred="$(event_count display_wake_inferred)"
    next_drawable_unavailable="$(event_count next_drawable_unavailable)"
    next_drawable_recovered="$(event_count next_drawable_recovered)"
    drawable_size_changed="$(event_count drawable_size_changed)"

    printf "  %-34s %s\n" "instance_guard_acquired" "$instance_guard_acquired"
    printf "  %-34s %s\n" "app_nap_guard_started" "$app_nap_guard_started"
    printf "  %-34s %s\n" "window_configured" "$window_configured"
    printf "  %-34s %s\n" "app_active_changed" "$app_active_changed"
    printf "  %-34s %s\n" "window_visible_changed" "$window_visible_changed"
    printf "  %-34s %s\n" "window_occlusion_changed" "$window_occlusion_changed"
    printf "  %-34s %s\n" "long_frame_gap" "$long_frame_gap"
    printf "  %-34s %s\n" "display_wake_inferred" "$display_wake_inferred"
    printf "  %-34s %s\n" "next_drawable_unavailable" "$next_drawable_unavailable"
    printf "  %-34s %s\n" "next_drawable_recovered" "$next_drawable_recovered"
    printf "  %-34s %s\n" "drawable_size_changed" "$drawable_size_changed"

    startup_status="PASS"
    startup_detail="startup guard, App Nap guard, and window configuration were logged"
    if (( instance_guard_acquired < 1 || app_nap_guard_started < 1 || window_configured < 1 )); then
      startup_status="RISK"
      startup_detail="expected startup guard/App Nap/window configuration events were missing"
    fi

    drawable_status="PASS"
    drawable_detail="drawable availability did not report an unrecovered loss"
    if (( next_drawable_unavailable > next_drawable_recovered )); then
      drawable_status="RISK"
      drawable_detail="next_drawable_unavailable is greater than next_drawable_recovered"
    fi

    wake_status="PASS"
    wake_detail="no inferred display wake was logged"
    if (( display_wake_inferred > 0 )); then
      wake_status="CHECK"
      wake_detail="display_wake_inferred appeared; manually confirm the avatar recovered"
    fi

    gap_status="PASS"
    gap_detail="no long frame gaps were logged"
    if (( long_frame_gap > 0 )); then
      gap_status="INFO"
      gap_detail="long_frame_gap is treated as a transition signal, not an automatic failure"
    fi

    recent_events="$(grep 'renderer_event=' "$LOG_PATH" | tail -20 || true)"
    if [[ -z "$recent_events" ]]; then
      recent_events="No renderer_event lines were recorded."
    fi

    {
      echo "# vtube-studio-rs Space Reliability Report"
      echo
      echo "- Generated: $(date '+%Y-%m-%d %H:%M:%S %z')"
      echo "- Model: \`$MODEL_PATH\`"
      echo "- Log: \`$LOG_PATH\`"
      echo
      echo "## Manual Checklist"
      echo
      echo "- [ ] Frames kept increasing during Space switches."
      echo "- [ ] FPS recovered to roughly 60 after transitions."
      echo "- [ ] Avatar window remained visible after Space switches."
      echo "- [ ] Avatar remained visible beside a full-screen app."
      echo "- [ ] Avatar recovered after display sleep/wake."
      echo "- [ ] No duplicate avatar windows appeared after reruns."
      echo "- [ ] Notes:"
      echo
      echo "## Event Counts"
      echo
      echo "| Event | Count |"
      echo "| --- | ---: |"
      echo "| instance_guard_acquired | $instance_guard_acquired |"
      echo "| app_nap_guard_started | $app_nap_guard_started |"
      echo "| window_configured | $window_configured |"
      echo "| app_active_changed | $app_active_changed |"
      echo "| window_visible_changed | $window_visible_changed |"
      echo "| window_occlusion_changed | $window_occlusion_changed |"
      echo "| long_frame_gap | $long_frame_gap |"
      echo "| display_wake_inferred | $display_wake_inferred |"
      echo "| next_drawable_unavailable | $next_drawable_unavailable |"
      echo "| next_drawable_recovered | $next_drawable_recovered |"
      echo "| drawable_size_changed | $drawable_size_changed |"
      echo
      echo "## Automatic Assessment"
      echo
      echo "| Check | Status | Detail |"
      echo "| --- | --- | --- |"
      status_line "Startup guards" "$startup_status" "$startup_detail"
      status_line "Drawable recovery" "$drawable_status" "$drawable_detail"
      status_line "Display wake" "$wake_status" "$wake_detail"
      status_line "Long frame gaps" "$gap_status" "$gap_detail"
      echo
      echo "## Recent Renderer Events"
      echo
      echo '```text'
      echo "$recent_events"
      echo '```'
    } >"$REPORT_PATH"

    echo
    echo "Recent renderer events:"
    echo "$recent_events"
    echo
    echo "Full log: $LOG_PATH"
    echo "Markdown report: $REPORT_PATH"
  else
    echo "  Log file was not created."
  fi
}

trap cleanup EXIT INT TERM
wait "$APP_PID"

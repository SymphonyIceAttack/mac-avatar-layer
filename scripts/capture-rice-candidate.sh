#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${1:-public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression/rice-candidate}"
CONFIG_PATH="$ROOT_DIR/vtube-studio-rs.toml"
EXAMPLE_CONFIG_PATH="$ROOT_DIR/vtube-studio-rs.example.toml"
BACKUP_PATH="$(mktemp "${TMPDIR:-/tmp}/vtube-studio-rs.toml.XXXXXX")"
HAD_CONFIG=0

if [[ -f "$CONFIG_PATH" ]]; then
  HAD_CONFIG=1
  cp "$CONFIG_PATH" "$BACKUP_PATH"
elif [[ -f "$EXAMPLE_CONFIG_PATH" ]]; then
  cp "$EXAMPLE_CONFIG_PATH" "$CONFIG_PATH"
else
  echo "Missing config template: $EXAMPLE_CONFIG_PATH" >&2
  exit 1
fi

restore_config() {
  if [[ "$HAD_CONFIG" == "1" ]]; then
    cp "$BACKUP_PATH" "$CONFIG_PATH"
  else
    rm -f "$CONFIG_PATH"
  fi
  rm -f "$BACKUP_PATH"
}
trap restore_config EXIT

set_renderer_mode() {
  local disable_masks="$1"
  local high_precision_masks="$2"
  perl -0pi \
    -e "s/^disable_masks = .*\$/disable_masks = ${disable_masks}/m;" \
    -e "s/^high_precision_masks = .*\$/high_precision_masks = ${high_precision_masks}/m;" \
    -e 's/^debug_texture_mode = .*$/debug_texture_mode = "none"/m;' \
    "$CONFIG_PATH"
}

capture_mode() {
  local label="$1"
  local disable_masks="$2"
  local high_precision_masks="$3"
  local model_name
  local output_path
  local latest_path

  model_name="$(basename "$MODEL_PATH" .model3.json)"
  latest_path="$OUTPUT_DIR/latest-${model_name}-${label}.png"

  set_renderer_mode "$disable_masks" "$high_precision_masks"
  echo "Capturing ${model_name} stress ${label}"
  output_path="$(OUTPUT_DIR="$OUTPUT_DIR" "$ROOT_DIR/scripts/capture-metal.sh" "$MODEL_PATH")"
  cp "$output_path" "$latest_path"
  echo "  $output_path"
  echo "  $latest_path"
}

mkdir -p "$OUTPUT_DIR"
if [[ "${VTUBE_RS_SKIP_TARGET_CLEAN:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/clean-target.sh" --generated
  mkdir -p "$OUTPUT_DIR"
fi

if [[ ! -f "$ROOT_DIR/$MODEL_PATH" && ! -f "$MODEL_PATH" ]]; then
  echo "Missing optional Rice stress model: $MODEL_PATH" >&2
  exit 1
fi

"$ROOT_DIR/scripts/probe-risk-models.sh" "$MODEL_PATH"

capture_mode "shared" "false" "false"
capture_mode "high-precision" "false" "true"
capture_mode "no-mask" "true" "false"

echo "Rice stress screenshots: $OUTPUT_DIR"
"$ROOT_DIR/scripts/render-regression-report-safe.sh"

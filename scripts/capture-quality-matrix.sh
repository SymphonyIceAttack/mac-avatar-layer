#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression/quality-matrix}"
CONFIG_PATH="$ROOT_DIR/vtube-studio-rs.toml"
EXAMPLE_CONFIG_PATH="$ROOT_DIR/vtube-studio-rs.example.toml"
BACKUP_PATH="$(mktemp "${TMPDIR:-/tmp}/vtube-studio-rs.toml.XXXXXX")"
HAD_CONFIG=0

if [[ "$#" -gt 0 ]]; then
  MODELS=("$@")
else
  MODELS=(
    "public/model/0.model3.json"
    "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json"
    "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json"
  )
fi

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

set_quality_mode() {
  local atlas_mipmaps="$1"
  perl -0pi \
    -e 's/^disable_masks = .*$/disable_masks = false/m;' \
    -e 's/^high_precision_masks = .*$/high_precision_masks = false/m;' \
    -e "s/^atlas_mipmaps = .*\$/atlas_mipmaps = ${atlas_mipmaps}/m;" \
    -e 's/^debug_texture_mode = .*$/debug_texture_mode = "none"/m;' \
    "$CONFIG_PATH"
}

capture_mode() {
  local model_path="$1"
  local label="$2"
  local atlas_mipmaps="$3"
  local model_name
  local output_path
  local latest_path

  model_name="$(basename "$model_path" .model3.json)"
  latest_path="$OUTPUT_DIR/latest-${model_name}-${label}.png"

  set_quality_mode "$atlas_mipmaps"
  echo "Capturing ${model_name} quality ${label}"
  output_path="$(OUTPUT_DIR="$OUTPUT_DIR" "$ROOT_DIR/scripts/capture-metal.sh" "$model_path")"
  cp "$output_path" "$latest_path"
  echo "  $output_path"
  echo "  $latest_path"
}

mkdir -p "$OUTPUT_DIR"
if [[ "${VTUBE_RS_SKIP_TARGET_CLEAN:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/clean-target.sh" --generated
  mkdir -p "$OUTPUT_DIR"
fi

for model in "${MODELS[@]}"; do
  if [[ ! -f "$ROOT_DIR/$model" && ! -f "$model" ]]; then
    echo "Skipping missing model: $model" >&2
    continue
  fi

  capture_mode "$model" "mipmaps-off" "false"
  capture_mode "$model" "mipmaps-on" "true"
done

echo "Quality matrix screenshots: $OUTPUT_DIR"
"$ROOT_DIR/scripts/render-regression-report-safe.sh"

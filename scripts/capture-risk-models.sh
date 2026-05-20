#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"

if [[ "$#" -gt 0 ]]; then
  MODELS=("$@")
else
  MODELS=(
    "public/model/0.model3.json"
    "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json"
    "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json"
  )
fi

mkdir -p "$OUTPUT_DIR"
if [[ "${VTUBE_RS_SKIP_TARGET_CLEAN:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/clean-target.sh" --generated
  mkdir -p "$OUTPUT_DIR"
fi

PROBE_MODELS=()
for model in "${MODELS[@]}"; do
  if [[ -f "$ROOT_DIR/$model" || -f "$model" ]]; then
    PROBE_MODELS+=("$model")
  fi
done

if [[ "${#PROBE_MODELS[@]}" -gt 0 ]]; then
  "$ROOT_DIR/scripts/probe-risk-models.sh" "${PROBE_MODELS[@]}"
else
  echo "Skipping model probe because no configured models were found." >&2
fi

for model in "${MODELS[@]}"; do
  if [[ ! -f "$ROOT_DIR/$model" && ! -f "$model" ]]; then
    echo "Skipping missing model: $model" >&2
    continue
  fi

  echo "Capturing $model"
  output_path="$(OUTPUT_DIR="$OUTPUT_DIR" "$ROOT_DIR/scripts/capture-metal.sh" "$model")"
  model_name="$(basename "$model" .model3.json)"
  latest_path="$OUTPUT_DIR/latest-${model_name}.png"
  cp "$output_path" "$latest_path"
  echo "  $output_path"
  echo "  $latest_path"
done

echo "Render regression screenshots: $OUTPUT_DIR"
echo "Render regression report: $("$ROOT_DIR/scripts/render-regression-report.sh")"

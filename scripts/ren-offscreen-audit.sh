#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
MODEL_PATH="${1:-public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json}"
PROBE_PATH="${PROBE_PATH:-$OUTPUT_DIR/ren-offscreen-audit-probe.txt}"
REPORT_PATH="${REPORT_PATH:-$OUTPUT_DIR/ren-offscreen-audit.md}"

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"

relative_path() {
  local path="$1"
  if [[ "$path" == "$ROOT_DIR/"* ]]; then
    printf '%s\n' "${path#"$ROOT_DIR/"}"
  else
    printf '%s\n' "$path"
  fi
}

write_risk_lines() {
  grep '^  risk ' "$PROBE_PATH" 2>/dev/null | sed 's/^  risk /- /' || true
}

write_offscreen_table() {
  echo "| Offscreen | Owner | Depth | Render | Blend | Opacity | Masks | Inverted Mask |"
  echo "| ---: | --- | ---: | ---: | --- | ---: | ---: | --- |"
  awk '
    /^  offscreen #/ {
      line = $0
      sub(/^  offscreen #/, "", line)
      split(line, id_parts, " owner ")
      id = id_parts[1]
      rest = id_parts[2]
      split(rest, owner_parts, " depth ")
      owner = owner_parts[1]
      rest = owner_parts[2]
      split(rest, depth_parts, " render ")
      depth = depth_parts[1]
      rest = depth_parts[2]
      split(rest, render_parts, " blend ")
      render = render_parts[1]
      rest = render_parts[2]
      split(rest, blend_parts, " opacity ")
      blend = blend_parts[1]
      rest = blend_parts[2]
      split(rest, opacity_parts, " multiply ")
      opacity = opacity_parts[1]
      rest = opacity_parts[2]
      split(rest, mask_parts, " masks ")
      rest = mask_parts[2]
      split(rest, inverted_parts, " inverted_mask=")
      masks = inverted_parts[1]
      inverted = inverted_parts[2]
      gsub(/\|/, "\\|", owner)
      gsub(/\|/, "\\|", blend)
      printf "| %s | `%s` | %s | %s | %s | %s | %s | %s |\n", id, owner, depth, render, blend, opacity, masks, inverted
    }
  ' "$PROBE_PATH"
}

write_extended_drawable_table() {
  echo "| Drawable | Part | Render | Blend | Opacity | Masks |"
  echo "| ---: | --- | ---: | --- | ---: | ---: |"
  awk '
    /^  drawable #/ && /blend Extended/ {
      line = $0
      sub(/^  drawable #/, "", line)
      split(line, id_parts, " ")
      drawable = id_parts[1]
      split(line, part_parts, " part ")
      rest = part_parts[2]
      split(rest, render_parts, " render ")
      part = render_parts[1]
      rest = render_parts[2]
      split(rest, blend_parts, " blend ")
      render = blend_parts[1]
      rest = blend_parts[2]
      split(rest, opacity_parts, " opacity ")
      blend = opacity_parts[1]
      rest = opacity_parts[2]
      split(rest, multiply_parts, " multiply ")
      opacity = multiply_parts[1]
      split(line, masks_parts, " masks ")
      masks = masks_parts[2]
      gsub(/\|/, "\\|", part)
      gsub(/\|/, "\\|", blend)
      printf "| %s | `%s` | %s | %s | %s | %s |\n", drawable, part, render, blend, opacity, masks
    }
  ' "$PROBE_PATH"
}

write_capture_references() {
  echo "| Mode | Screenshot | Log |"
  echo "| --- | --- | --- |"
  local log_path="$OUTPUT_DIR/offscreen-matrix/capture.log"
  for mode in shared high-precision no-mask; do
    local image="$OUTPUT_DIR/offscreen-matrix/latest-Ren-${mode}.png"
    if [[ -f "$image" ]]; then
      printf '| %s | `%s` | `%s` |\n' "$mode" "$(relative_path "$image")" "$(relative_path "$log_path")"
    else
      printf '| %s | _Missing_ | `%s` |\n' "$mode" "$(relative_path "$log_path")"
    fi
  done
}

PROBE_PATH="$PROBE_PATH" "$ROOT_DIR/scripts/probe-risk-models.sh" "$MODEL_PATH" >/dev/null

{
  echo "# Ren Offscreen / Extended Blend Audit"
  echo
  echo "- Generated: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- Model: \`$MODEL_PATH\`"
  echo "- Probe: \`$(relative_path "$PROBE_PATH")\`"
  echo
  echo "## Audit Focus"
  echo
  echo "- [ ] Offscreen render order matches the owner part order expected by Cubism Framework."
  echo "- [ ] Nested offscreens composite after descendants and before parent target flush."
  echo "- [ ] Masked offscreens apply the same mask matrix path as masked drawables."
  echo "- [ ] Extended offscreens and extended drawables sample the correct pre-composite snapshot."
  echo "- [ ] High-precision mask fallback is expected while offscreen rendering is active."
  echo
  echo "## Risk Summary"
  echo
  write_risk_lines
  echo
  echo "## Offscreen Objects"
  echo
  write_offscreen_table
  echo
  echo "## Extended Drawable Objects"
  echo
  write_extended_drawable_table
  echo
  echo "## Capture References"
  echo
  write_capture_references
  echo
  echo "## Raw Probe"
  echo
  echo '```text'
  sed -n '1,260p' "$PROBE_PATH"
  echo '```'
} >"$REPORT_PATH"

echo "$REPORT_PATH"

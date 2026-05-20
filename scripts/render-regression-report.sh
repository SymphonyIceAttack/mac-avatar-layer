#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
REPORT_PATH="${REPORT_PATH:-$OUTPUT_DIR/report.md}"

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

report_image_path() {
  local path="$1"
  if [[ "$path" == "$OUTPUT_DIR/"* ]]; then
    printf '%s\n' "${path#"$OUTPUT_DIR/"}"
  else
    relative_path "$path"
  fi
}

write_latest_table() {
  local title="$1"
  local directory="$2"
  local note="$3"
  local found=0

  echo "## $title"
  echo
  echo "$note"
  echo
  echo "| Preview | Screenshot | Path |"
  echo "| --- | --- | --- |"

  while IFS= read -r image; do
    found=1
    local relative
    local preview
    relative="$(relative_path "$image")"
    preview="$(report_image_path "$image")"
    printf '| <img src="%s" width="160"> | %s | `%s` |\n' "$preview" "$(basename "$image")" "$relative"
  done < <(find "$directory" -maxdepth 1 -type f -name 'latest-*.png' 2>/dev/null | sort)

  if [[ "$found" == "0" ]]; then
    printf '|  | _No latest screenshots found_ | `%s` |\n' "$(relative_path "$directory")"
  fi
  echo
}

write_contact_sheet_group() {
  local title="$1"
  local directory="$2"
  local note="$3"
  local found=0

  echo "### $title"
  echo
  echo "$note"
  echo

  while IFS= read -r image; do
    found=1
    local preview
    preview="$(report_image_path "$image")"
    printf '<figure style="display:inline-block;margin:0 12px 18px 0;vertical-align:top;width:180px;">\n'
    printf '  <img src="%s" width="180">\n' "$preview"
    printf '  <figcaption><code>%s</code></figcaption>\n' "$(basename "$image")"
    printf '</figure>\n'
  done < <(find "$directory" -maxdepth 1 -type f -name 'latest-*.png' 2>/dev/null | sort)

  if [[ "$found" == "0" ]]; then
    printf '_No latest screenshots found in `%s`._\n' "$(relative_path "$directory")"
  fi
  echo
}

write_contact_sheet() {
  echo "## Visual Contact Sheet"
  echo
  echo "Use this section for fast visual triage before opening individual PNG files."
  echo
  write_contact_sheet_group \
    "Risk Model Sweep" \
    "$OUTPUT_DIR" \
    "Broad baseline captures for the default model, Mao, and Ren."
  write_contact_sheet_group \
    "Mao Mask Matrix" \
    "$OUTPUT_DIR/mask-matrix" \
    "Shared, high-precision, and no-mask clipping comparison."
  write_contact_sheet_group \
    "Ren Offscreen Matrix" \
    "$OUTPUT_DIR/offscreen-matrix" \
    "Shared, high-precision fallback, and no-mask offscreen comparison."
  write_contact_sheet_group \
    "Rice Optional Stress Matrix" \
    "$OUTPUT_DIR/rice-candidate" \
    "Optional stress coverage for additive, inverted-mask, and translucent drawable behavior."
  write_contact_sheet_group \
    "Quality Matrix" \
    "$OUTPUT_DIR/quality-matrix" \
    "Texture atlas mipmaps off/on comparison."
}

write_capture_log_summary() {
  echo "## Capture Log Summaries"
  echo
  echo "| Directory | Last Renderer Events |"
  echo "| --- | --- |"

  local found=0
  while IFS= read -r log_path; do
    found=1
    local relative
    local events
    relative="$(relative_path "$log_path")"
    events="$(grep 'renderer_event=' "$log_path" 2>/dev/null | tail -5 | sed 's/|/\\|/g' | awk 'NR == 1 { printf "%s", $0; next } { printf "<br>%s", $0 }')"
    if [[ -z "$events" ]]; then
      events="No renderer_event lines recorded."
    fi
    printf '| `%s` | %s |\n' "$relative" "$events"
  done < <(find "$OUTPUT_DIR" -maxdepth 2 -type f -name 'capture.log' 2>/dev/null | sort)

  if [[ "$found" == "0" ]]; then
    echo "| _No capture logs found_ | Run a capture script first. |"
  fi
  echo
}

write_review_focus() {
  local probe_path="$OUTPUT_DIR/probe.txt"
  local has_probe=0
  [[ -f "$probe_path" ]] && has_probe=1

  probe_risks_for_model() {
    local model="$1"
    awk -v model="$model" '
      $0 ~ model && $1 ~ /^public\// { capture = 1; next }
      capture && $1 ~ /^public\// { exit }
      capture && /^Live2D/ { exit }
      capture && /^  risk / {
        sub(/^  risk /, "")
        gsub(/\|/, "\\|")
        if (count > 0) {
          printf "<br>"
        }
        printf "%s", $0
        count++
      }
    ' "$probe_path"
  }

  echo "## Review Focus"
  echo
  echo "| Area | Why It Matters | Screenshots To Check |"
  echo "| --- | --- | --- |"

  if [[ "$has_probe" == "1" && "$(grep -c 'Mao.model3.json' "$probe_path" 2>/dev/null || true)" -gt 0 ]]; then
    local mao_reasons
    mao_reasons="$(probe_risks_for_model 'Mao.model3.json')"
    [[ -z "$mao_reasons" ]] && mao_reasons="High-mask sample model present in probe."
    printf '| Mao clipping | %s | `target/render-regression/mask-matrix/latest-Mao-shared.png`<br>`target/render-regression/mask-matrix/latest-Mao-high-precision.png`<br>`target/render-regression/mask-matrix/latest-Mao-no-mask.png` |\n' "$mao_reasons"
  fi

  if [[ "$has_probe" == "1" && "$(grep -c 'Ren.model3.json' "$probe_path" 2>/dev/null || true)" -gt 0 ]]; then
    local ren_reasons
    ren_reasons="$(probe_risks_for_model 'Ren.model3.json')"
    [[ -z "$ren_reasons" ]] && ren_reasons="Offscreen sample model present in probe."
    printf '| Ren offscreen/extended blend | %s | `target/render-regression/offscreen-matrix/latest-Ren-shared.png`<br>`target/render-regression/offscreen-matrix/latest-Ren-high-precision.png`<br>`target/render-regression/offscreen-matrix/latest-Ren-no-mask.png` |\n' "$ren_reasons"
  fi

  if [[ "$has_probe" == "1" && "$(grep -c 'Rice.model3.json' "$probe_path" 2>/dev/null || true)" -gt 0 ]]; then
    local rice_reasons
    rice_reasons="$(probe_risks_for_model 'Rice.model3.json')"
    [[ -z "$rice_reasons" ]] && rice_reasons="Optional Rice stress model present in probe."
    printf '| Rice optional stress | %s | `target/render-regression/rice-candidate/latest-Rice-shared.png`<br>`target/render-regression/rice-candidate/latest-Rice-high-precision.png`<br>`target/render-regression/rice-candidate/latest-Rice-no-mask.png` |\n' "$rice_reasons"
  fi

  if [[ "$has_probe" == "1" && "$(grep -c 'public/model/0.model3.json' "$probe_path" 2>/dev/null || true)" -gt 0 ]]; then
    local default_reasons
    default_reasons="$(probe_risks_for_model 'public/model/0.model3.json')"
    [[ -z "$default_reasons" ]] && default_reasons="Local default model baseline."
    printf '| Default model baseline | %s | `target/render-regression/latest-0.png`<br>`target/render-regression/quality-matrix/latest-0-mipmaps-off.png`<br>`target/render-regression/quality-matrix/latest-0-mipmaps-on.png` |\n' "$default_reasons"
  fi

  local fallback_events
  fallback_events="$(find "$OUTPUT_DIR" -maxdepth 2 -type f -name 'capture.log' -exec grep -h 'renderer_event=high_precision_mask_fallback' {} + 2>/dev/null | sed 's/|/\\|/g' | awk 'NR == 1 { printf "%s", $0; next } { printf "<br>%s", $0 }' || true)"
  if [[ -n "$fallback_events" ]]; then
    printf '| High precision mask fallback | %s | `target/render-regression/offscreen-matrix/latest-Ren-high-precision.png`<br>`target/render-regression/fallback-diagnostics-smoke/report.md` |\n' "$fallback_events"
  fi

  if find "$OUTPUT_DIR/quality-matrix" -maxdepth 1 -type f -name 'latest-*-mipmaps-on.png' >/dev/null 2>&1; then
    echo '| Texture sampling | Mipmaps-on captures are present; check for atlas island bleed or unexpected blur. | `target/render-regression/quality-matrix/latest-0-mipmaps-on.png`<br>`target/render-regression/quality-matrix/latest-Mao-mipmaps-on.png`<br>`target/render-regression/quality-matrix/latest-Ren-mipmaps-on.png` |'
  fi

  if [[ "$has_probe" != "1" ]]; then
    echo '| Probe missing | Run `./scripts/probe-risk-models.sh` or `./scripts/capture-risk-models.sh` to populate model-specific review focus. | `target/render-regression/probe.txt` |'
  fi
  echo
}

write_probe_summary() {
  local probe_path="$OUTPUT_DIR/probe.txt"

  echo "## Model Risk Probe"
  echo
  if [[ ! -f "$probe_path" ]]; then
    echo "No probe output found. Run \`./scripts/probe-risk-models.sh\` or \`./scripts/capture-risk-models.sh\`."
    echo
    return
  fi

  echo "Source: \`$(relative_path "$probe_path")\`"
  echo
  echo '```text'
  sed -n '1,220p' "$probe_path"
  echo '```'
  echo
}

write_fallback_summary() {
  echo "## Renderer Fallbacks"
  echo
  echo "| Log | Fallback Events |"
  echo "| --- | --- |"

  local found=0
  while IFS= read -r log_path; do
    local events
    events="$(grep 'renderer_event=high_precision_mask_fallback' "$log_path" 2>/dev/null | sed 's/|/\\|/g' | awk 'NR == 1 { printf "%s", $0; next } { printf "<br>%s", $0 }' || true)"
    if [[ -n "$events" ]]; then
      found=1
      printf '| `%s` | %s |\n' "$(relative_path "$log_path")" "$events"
    fi
  done < <(find "$OUTPUT_DIR" -maxdepth 2 -type f -name 'capture.log' 2>/dev/null | sort)

  if [[ "$found" == "0" ]]; then
    echo '| _No fallback events found_ | No `high_precision_mask_fallback` events recorded in capture logs. |'
  fi
  echo
}

write_ren_audit_summary() {
  local audit_path="$OUTPUT_DIR/ren-offscreen-audit.md"

  echo "## Ren Offscreen Audit"
  echo
  if [[ ! -f "$audit_path" ]]; then
    echo "No Ren audit found. Run \`./scripts/ren-offscreen-audit.sh\`."
    echo
    return
  fi

  echo "Source: \`$(relative_path "$audit_path")\`"
  echo
  awk '/^## Raw Probe/ { exit } { print }' "$audit_path"
  echo
}

{
  echo "# vtube-studio-rs Render Regression Report"
  echo
  echo "- Generated: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- Root: \`$ROOT_DIR\`"
  echo "- Output: \`$(relative_path "$OUTPUT_DIR")\`"
  echo
  echo "## Manual Checklist"
  echo
  echo "- [ ] Default model looks complete and correctly layered."
  echo "- [ ] Mao shared/high-precision/no-mask screenshots show expected clipping differences."
  echo "- [ ] Ren shared/high-precision/no-mask screenshots preserve offscreen composites."
  echo "- [ ] Rice optional stress screenshots, when present, do not reveal new additive, inverted-mask, or translucent artifacts."
  echo "- [ ] Mipmaps-on screenshots do not show obvious atlas island bleed."
  echo "- [ ] Mipmaps-off screenshots remain crisp without severe shimmer in static capture."
  echo "- [ ] Notes:"
  echo
  write_contact_sheet
  write_latest_table \
    "Risk Model Sweep" \
    "$OUTPUT_DIR" \
    "Default model plus high-risk sample captures. Use these for broad regressions."
  write_latest_table \
    "Mao Mask Matrix" \
    "$OUTPUT_DIR/mask-matrix" \
    "Compare shared, high-precision, and no-mask clipping behavior."
  write_latest_table \
    "Ren Offscreen Matrix" \
    "$OUTPUT_DIR/offscreen-matrix" \
    "Compare shared, high-precision fallback, and no-mask offscreen behavior."
  write_latest_table \
    "Rice Optional Stress Matrix" \
    "$OUTPUT_DIR/rice-candidate" \
    "Optional stress coverage for additive, inverted-mask, and translucent drawable behavior."
  write_latest_table \
    "Quality Matrix" \
    "$OUTPUT_DIR/quality-matrix" \
    "Compare texture atlas mipmaps off/on for shimmer and atlas bleed."
  write_review_focus
  write_probe_summary
  write_fallback_summary
  write_ren_audit_summary
  write_capture_log_summary
  echo "## Interpretation Guide"
  echo
  echo "- Mask regressions usually show up first in the Mao matrix."
  echo "- Offscreen and extended blend regressions usually show up first in the Ren matrix."
  echo "- Mipmap regressions usually appear as blurry details or color bleed from neighboring atlas islands."
  echo "- If a capture is missing, run the corresponding \`scripts/capture-*.sh\` script and regenerate this report."
} >"$REPORT_PATH"

echo "$REPORT_PATH"

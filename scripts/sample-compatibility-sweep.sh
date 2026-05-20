#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/render-regression}"
SAMPLES_ROOT="${1:-public/CubismSdkForNative/Samples/Resources}"
PROBE_PATH="${PROBE_PATH:-$OUTPUT_DIR/compatibility-probe.txt}"
REPORT_PATH="${REPORT_PATH:-$OUTPUT_DIR/compatibility-sweep.md}"

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"

"$ROOT_DIR/scripts/probe-risk-models.sh" "$SAMPLES_ROOT" >/dev/null
cp "$OUTPUT_DIR/probe.txt" "$PROBE_PATH"

relative_path() {
  local path="$1"
  if [[ "$path" == "$ROOT_DIR/"* ]]; then
    printf '%s\n' "${path#"$ROOT_DIR/"}"
  else
    printf '%s\n' "$path"
  fi
}

write_model_table() {
  awk '
    function risk_rank(value) {
      if (value == "risk:high") {
        return 3
      }
      if (value == "risk:medium") {
        return 2
      }
      if (value == "risk:low") {
        return 1
      }
      return 0
    }
    function reset_model() {
      if (model != "") {
        if (reasons == "") {
          reasons = "No specific risk lines."
        }
        printf "%d|%s|%s|%s|%s|| `%s` | %s | %s | %s | %s | %s | %s |\n", risk_rank(status), off, ext, max_mask, masks, model, status, masks, max_mask, ext, off, reasons
      }
      model = ""
      status = ""
      masks = ""
      max_mask = ""
      ext = ""
      off = ""
      reasons = ""
    }
    $1 ~ /\.model3\.json$/ {
      reset_model()
      model = $1
      masks = $5
      max_mask = $6
      ext = $9
      off = $11
      status = $NF
      next
    }
    /^  risk / {
      sub(/^  risk /, "")
      gsub(/\|/, "\\|")
      if (reasons != "") {
        reasons = reasons "<br>" $0
      } else {
        reasons = $0
      }
    }
    END {
      reset_model()
    }
  ' "$PROBE_PATH" | sort -t '|' -k1,1nr -k2,2nr -k3,3nr -k4,4nr -k5,5nr | cut -d '|' -f6-
}

write_recommendations() {
  local high_count
  local offscreen_count
  local extended_count
  local dense_count
  high_count="$(grep -c 'risk:high' "$PROBE_PATH" 2>/dev/null || true)"
  offscreen_count="$(grep -c '^  risk offscreen objects:' "$PROBE_PATH" 2>/dev/null || true)"
  extended_count="$(grep -c '^  risk extended blend objects:' "$PROBE_PATH" 2>/dev/null || true)"
  dense_count="$(grep -c '^  risk dense clipping:' "$PROBE_PATH" 2>/dev/null || true)"

  echo "## Recommendations"
  echo
  echo "- High-risk models found: $high_count."
  echo "- Models with offscreen objects: $offscreen_count."
  echo "- Models with extended blend objects: $extended_count."
  echo "- Models with dense clipping: $dense_count."
  echo '- Keep `Mao` in the mask matrix while it remains the dense clipping stress model.'
  echo '- Keep `Ren` in the offscreen matrix while it remains the offscreen/extended blend stress model.'
  echo '- Add another model to a screenshot matrix only when this sweep shows a new risk shape not covered by `Mao` or `Ren`.'
  echo
}

{
  echo "# vtube-studio-rs Sample Compatibility Sweep"
  echo
  echo "- Generated: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- Samples root: \`$SAMPLES_ROOT\`"
  echo "- Probe: \`$(relative_path "$PROBE_PATH")\`"
  echo
  write_recommendations
  echo "## Model Risk Table"
  echo
  echo "| Model | Risk | Masks | Max Mask | Ext Blend | Offscreens | Reasons |"
  echo "| --- | --- | ---: | ---: | ---: | ---: | --- |"
  write_model_table
  echo
  echo "## Raw Probe"
  echo
  echo '```text'
  sed -n '1,260p' "$PROBE_PATH"
  echo '```'
} >"$REPORT_PATH"

echo "$REPORT_PATH"

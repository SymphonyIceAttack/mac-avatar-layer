#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_TIMEOUT_SECONDS="${REPORT_TIMEOUT_SECONDS:-20}"

if [[ "${VTUBE_RS_SKIP_REPORT:-0}" == "1" ]]; then
  echo "Render regression report: skipped (VTUBE_RS_SKIP_REPORT=1)"
  exit 0
fi

if output="$(perl -e 'my $timeout = shift; alarm $timeout; exec @ARGV' \
  "$REPORT_TIMEOUT_SECONDS" \
  "$ROOT_DIR/scripts/render-regression-report.sh" 2>&1)"; then
  echo "Render regression report: $output"
else
  status="$?"
  echo "Render regression report: skipped after ${REPORT_TIMEOUT_SECONDS}s or failed with status ${status}" >&2
  if [[ -n "${output:-}" ]]; then
    echo "$output" >&2
  fi
fi

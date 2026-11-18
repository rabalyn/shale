#!/bin/bash
# Makes docs/perf/index.html from every docs/perf/results/*.json (and results-worst/*.json) and
# the flamegraphs in docs/perf/flamegraphs/ (BUILD-032, raba/shale#343): a self-contained page
# with the latest run as a table and the history of every scenario as charts.
#
#   scripts/bench-report.sh [--no-build] [--perf-dir DIR]
set -euo pipefail
cd "$(dirname "$0")/.."
BUILD=yes DIR=docs/perf
while [ $# -gt 0 ]; do
  case $1 in
    --no-build) BUILD=no ;;
    --perf-dir) shift; DIR=${1:-} ;;
    -h|--help) sed -n '2,/^set -euo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "bench-report.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
BIN=${CARGO_TARGET_DIR:-target}/bench-release/shale
[ "$BUILD" = no ] || cargo build --profile bench-release --features bench --locked
[ -x "$BIN" ] || { echo "bench-report.sh: $BIN is missing (build it without --no-build)" >&2; exit 1; }
"$BIN" --bench-report "$DIR" "$DIR/index.html"

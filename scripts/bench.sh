#!/bin/bash
# Runs the benchmark harness (BUILD-027…BUILD-033, raba/shale#343) and, with --flamegraph, makes a
# flamegraph of every scenario. See docs/perf/README.md.
#
#   scripts/bench.sh [--profile quick|full|worst] [--filter IDS] [--iters N]
#                    [--out FILE] [--flamegraph | --flamegraph-only] [--flame-dir DIR] [--keep-folded]
#                    [--no-build] [--bin PROGRAM]
#
# Builds `cargo build --profile bench-release --features bench` (the release profile with line
# tables; the shipped release is unchanged), runs `shale --bench` headless and writes
# target/bench/<short commit>.json (`-quick` or `-worst` added for those profiles; --out says
# another file). --filter takes parts of IDs, or `=id` for exactly one, separated by commas. With --flamegraph each scenario of the profile (or those of --filter) runs
# on its own under `perf record -e cpu-clock:u -F 999 --call-graph fp --user-callchains`, only the measured loop is kept, and
# docs/perf/flamegraphs/<id>.svg is made with inferno (`cargo install inferno`). Without perf, or
# without the permission to use it (kernel.perf_event_paranoid), or without inferno it says so and
# goes on without flamegraphs. --flamegraph-only makes the flamegraphs without the normal run.
# --keep-folded also keeps the folded stacks (<id>.folded next to the svg: scripts/bench-compare.sh
# --flamegraphs diffs them). --bin runs another benchmark program (an older commit's build; no
# build is done). Environment: CARGO_TARGET_DIR, FLAME_MIN_MS (how long a scenario is measured for
# its flamegraph, default 3000), SHALE_BENCH_COMMIT and SHALE_BENCH_DESCRIBE (else read from git).
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=full FILTER="" ITERS="" OUT="" FLAME=no RUN=yes FLAME_DIR=docs/perf/flamegraphs BUILD=yes BIN="" KEEP=no
while [ $# -gt 0 ]; do
  case $1 in
    --profile) shift; PROFILE=${1:-} ;;
    --filter) shift; FILTER=${1:-} ;;
    --iters) shift; ITERS=${1:-} ;;
    --out) shift; OUT=${1:-} ;;
    --flamegraph) FLAME=yes ;;
    --flamegraph-only) FLAME=yes RUN=no ;;
    --flame-dir) shift; FLAME_DIR=${1:-} ;;
    --no-build) BUILD=no ;;
    --bin) shift; BIN=${1:-}; BUILD=no ;;
    --keep-folded) KEEP=yes ;;
    -h|--help) sed -n '2,/^set -euo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "bench.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
case $PROFILE in quick|full|worst) ;; *) echo "bench.sh: --profile is quick, full or worst, not $PROFILE" >&2; exit 2 ;; esac

. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/bench-env.sh"
TARGET=${CARGO_TARGET_DIR:-target}
[ -n "$BIN" ] || BIN="$TARGET/bench-release/shale"
if [ "$BUILD" = yes ]; then
  cargo build --profile bench-release --features bench --locked
fi
[ -x "$BIN" ] || { echo "bench.sh: $BIN is missing (build it: cargo build --profile bench-release --features bench)" >&2; exit 1; }

SHORT=$(git rev-parse --short HEAD)
export SHALE_BENCH_COMMIT=${SHALE_BENCH_COMMIT:-$(git rev-parse HEAD)}
export SHALE_BENCH_DESCRIBE=${SHALE_BENCH_DESCRIBE:-$(git describe --tags --always --dirty)}
export SHALE_BENCH_PROFILE=bench-release
if [ -z "$OUT" ]; then
  suffix=""
  [ "$PROFILE" = full ] || suffix="-$PROFILE"
  OUT="target/bench/$SHORT$suffix.json"
fi
mkdir -p "$(dirname "$OUT")"

args=(--bench "$OUT" --profile "$PROFILE")
[ -z "$FILTER" ] || args+=(--filter "$FILTER")
[ -z "$ITERS" ] || args+=(--iters "$ITERS")
if [ "$RUN" = yes ]; then
  "$BIN" "${args[@]}"
  echo "result: $OUT"
fi

[ "$FLAME" = yes ] || exit 0

# ---- flamegraphs ----
command -v perf >/dev/null || { echo "bench.sh: perf is not installed: no flamegraphs" >&2; exit 0; }
for tool in inferno-collapse-perf inferno-flamegraph; do
  command -v "$tool" >/dev/null || { echo "bench.sh: $tool is missing: cargo install inferno (no flamegraphs)" >&2; exit 0; }
done
paranoid=$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 0)
if [ "$paranoid" -gt 2 ]; then
  echo "bench.sh: kernel.perf_event_paranoid is $paranoid: perf may not profile this process (sysctl kernel.perf_event_paranoid=2); no flamegraphs" >&2
  exit 0
fi
mkdir -p "$FLAME_DIR"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
MIN_MS=${FLAME_MIN_MS:-3000}
# --filter as the harness reads it: parts of IDs or `=id`, separated by commas
ids=$("$BIN" --bench-list | awk -v p="$PROFILE" -v f="$FILTER" '
  BEGIN { n = split(f, parts, ",") }
  $1 == p { hit = (f == ""); for (i = 1; i <= n; i++) { q = parts[i]; if (q == "") continue; if (substr(q, 1, 1) == "=" ? $2 == substr(q, 2) : index($2, q)) hit = 1 } if (hit) print $2 }')
made=0
for id in $ids; do
  echo "-- flamegraph $id"
  if ! perf record -q -e cpu-clock:u -F 999 --call-graph fp --user-callchains -o "$TMP/perf.data" -- \
      "$BIN" --bench "$TMP/run.json" --profile "$PROFILE" --filter "=$id" --min-ms "$MIN_MS" >/dev/null 2>"$TMP/perf.err"; then
    echo "bench.sh: perf could not profile $id: $(tail -n 1 "$TMP/perf.err")" >&2
    continue
  fi
  # only the stacks through the measured loop: not the setup, not the vault generation
  # inferno cannot read a program path with a space in it (and the repository may be in one): only the file name is kept;
  # inlined functions are left out (--no-inline; a call site is otherwise given the inlined code that follows it, and resolving them is slow)
  perf script --no-inline -i "$TMP/perf.data" 2>/dev/null | sed -E 's#\((/[^)]*/)([^/)]*)\)$#(\2)#' | inferno-collapse-perf -q \
    | grep 'measured_region' | sed -E 's/^.*;([^;]*measured_region)/\1/' > "$TMP/folded.txt" || true
  if [ ! -s "$TMP/folded.txt" ]; then
    echo "bench.sh: no samples of the measured loop for $id (too short or skipped)" >&2
    continue
  fi
  [ "$KEEP" = no ] || cp "$TMP/folded.txt" "$FLAME_DIR/$id.folded"
  inferno-flamegraph --title "$id" --subtitle "$SHALE_BENCH_DESCRIBE, perf 999 Hz" < "$TMP/folded.txt" > "$FLAME_DIR/$id.svg" 2>/dev/null
  made=$((made + 1))
done
echo "flamegraphs: $made in $FLAME_DIR"

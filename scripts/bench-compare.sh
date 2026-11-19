#!/bin/bash
# Benchmarks two commits and compares them (BUILD-030, BUILD-036, raba/shale#343). For an agent or a release.
#
#   scripts/bench-compare.sh <commit-X> <commit-Y> [--profile quick|full|worst] [--runs N]
#                            [--filter IDS] [--changed IDS] [--threshold PCT] [--bytes-threshold PCT]
#                            [--allocs-threshold PCT] [--max-load LOAD] [--max-wait SECONDS]
#                            [--flamegraphs] [--flame-max N] [--out DIR]
#
# X is the older commit, Y the newer one. Each gets a temporary git worktree (removed at the end),
# is built with `--profile bench-release --features bench` and run by its own binary; a commit
# without the benchmark harness (no `bench` feature) is an error. The comparison itself is made
# by the harness of the checkout this script is in (a third build, unless it is Y's), so X and Y
# may be older than `--bench-merge` and the scenario versions.
#
# A busy computer only makes times larger, so each side runs --runs times (default 3, `worst` 2),
# interleaved X, Y, Y, X, X, Y ... so a busy minute hits both sides alike; the runs of a side are
# merged (`--bench-merge`: the smallest median is the time, the range of the medians is the spread).
# A time counts only when it differs by more than the threshold and the spreads do not overlap;
# allocation counts and peak bytes are exact and count on their own (docs/perf/README.md).
#
# DIR (default target/bench-compare-out) gets x.json and y.json (the merged results), runs/ (every
# single run), compare.md and compare.json (verdict ok|faster|regression|error, the regressions
# and improvements worst and best first, the noisy and changed scenarios, the skipped ones, the
# load), and with --flamegraphs flamegraphs/x, flamegraphs/y and flamegraphs/diff of the scenarios
# in compare.json's regressions, noisy and best improvements (--flame-max, default 6; needs perf,
# inferno and jq; never written into git).
#
# Exit code 0: no regression (ok or faster); 1: a regression (thresholds: median time 10 %,
# allocations 10 %, peak bytes 15 %), listed on stdout; 2: an error (a commit without the harness,
# a build or run that failed; compare.json then says verdict error).
# --filter takes IDs or parts of IDs separated by commas (`=id` for exactly one) and measures only
# those: for running again what the first run flagged. --changed names scenarios whose meaning
# differs between X and Y although the results carry no version (commits older than BUILD-036).
# --max-load waits (up to --max-wait seconds, default 600) before each run until the one-minute
# load average is below LOAD, then runs anyway. CARGO_TARGET_DIR (default target/bench-compare)
# is shared by the builds.
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2

X="" Y="" PROFILE=full RUNS="" TIME_PCT=10 BYTES_PCT=15 ALLOCS_PCT=10 OUT=target/bench-compare-out
FILTER="" CHANGED="" MAXLOAD="" MAXWAIT=600 FLAMES=no FLAMEMAX=6
need() { [ -n "${2:-}" ] || { echo "bench-compare.sh: $1 needs a value" >&2; exit 2; }; }
while [ $# -gt 0 ]; do
  case $1 in
    --profile) need "$1" "${2:-}"; shift; PROFILE=$1 ;;
    --runs) need "$1" "${2:-}"; shift; RUNS=$1 ;;
    --filter) need "$1" "${2:-}"; shift; FILTER=$1 ;;
    --changed) need "$1" "${2:-}"; shift; CHANGED=${CHANGED:+$CHANGED,}$1 ;;
    --threshold) need "$1" "${2:-}"; shift; TIME_PCT=$1 ;;
    --bytes-threshold) need "$1" "${2:-}"; shift; BYTES_PCT=$1 ;;
    --allocs-threshold) need "$1" "${2:-}"; shift; ALLOCS_PCT=$1 ;;
    --max-load) need "$1" "${2:-}"; shift; MAXLOAD=$1 ;;
    --max-wait) need "$1" "${2:-}"; shift; MAXWAIT=$1 ;;
    --flamegraphs) FLAMES=yes ;;
    --flame-max) need "$1" "${2:-}"; shift; FLAMEMAX=$1 ;;
    --out) need "$1" "${2:-}"; shift; OUT=$1 ;;
    -h|--help) sed -n '2,/^set -uo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "bench-compare.sh: unknown option $1" >&2; exit 2 ;;
    *) if [ -z "$X" ]; then X=$1; elif [ -z "$Y" ]; then Y=$1; else echo "bench-compare.sh: two commits only" >&2; exit 2; fi ;;
  esac
  shift
done
if [ -z "$Y" ]; then
  echo "usage: scripts/bench-compare.sh <commit-X> <commit-Y> [--profile P] [--runs N] [--filter IDS] [--changed IDS] [--threshold PCT] [--bytes-threshold PCT] [--allocs-threshold PCT] [--max-load LOAD] [--flamegraphs] [--out DIR]" >&2
  exit 2
fi
case $PROFILE in quick|full|worst) ;; *) echo "bench-compare.sh: --profile is quick, full or worst, not $PROFILE" >&2; exit 2 ;; esac
if [ -z "$RUNS" ]; then
  RUNS=3
  [ "$PROFILE" != worst ] || RUNS=2
fi
case $RUNS in ''|*[!0-9]*|0) echo "bench-compare.sh: --runs needs a number of at least 1, not $RUNS" >&2; exit 2 ;; esac

TMP=$(mktemp -d)
cleanup() {
  for w in "$TMP"/x "$TMP"/y; do
    [ -e "$w" ] && git worktree remove --force "$w" >/dev/null 2>&1
  done
  git worktree prune
  rm -rf "$TMP"
}
trap cleanup EXIT
trap 'exit 2' INT TERM
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/bench-env.sh"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target/bench-compare}
mkdir -p "$OUT/runs" "$TMP/bin"
OUT=$(cd "$OUT" && pwd)
rm -f "$OUT"/runs/*.json "$OUT"/compare.json "$OUT"/compare.md

# fail <message>: say it, write compare.json with verdict error, exit 2
fail() {
  echo "bench-compare.sh: $1" >&2
  printf '{"schema":2,"verdict":"error","exit_code":2,"error":"%s"}\n' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')" > "$OUT/compare.json"
  printf '# Benchmark comparison: error\n\n%s\n' "$1" > "$OUT/compare.md"
  exit 2
}

# prepare <name x|y> <rev>: the worktree and the build; the binary is kept in $TMP/bin/<name>
prepare() {
  local name=$1 rev=$2 full
  full=$(git rev-parse --verify -q "$rev^{commit}") || fail "$rev is not a commit"
  git worktree add -q --detach "$TMP/$name" "$full" || fail "no worktree for $rev"
  if ! git -C "$TMP/$name" show "HEAD:Cargo.toml" | command grep -q '^bench = ' || [ ! -d "$TMP/$name/src/bench" ]; then
    fail "commit $rev has no benchmark harness (no feature \`bench\`; it came with raba/shale#343), so it cannot be measured"
  fi
  echo "== building $rev ($(git rev-parse --short "$full"))"
  (cd "$TMP/$name" && cargo build --profile bench-release --features bench --locked) || fail "the build of $rev failed"
  cp "$CARGO_TARGET_DIR/bench-release/shale" "$TMP/bin/$name"
  eval "FULL_$name=$full"
}

# the harness of this checkout makes the comparison: Y's own binary when Y is this checkout, else a build of it
prepare_tool() {
  if [ "$(git rev-parse HEAD)" = "$FULL_y" ] && git diff --quiet HEAD -- . 2>/dev/null; then
    cp "$TMP/bin/y" "$TMP/bin/tool"
  else
    # a folder of its own: cargo takes a file older than the last build in the shared folder for unchanged (the worktrees' files are new, this checkout's are not)
    echo "== building the comparison tool (this checkout)"
    CARGO_TARGET_DIR="$CARGO_TARGET_DIR-tool" cargo build --profile bench-release --features bench --locked || fail "the build of the comparison tool failed"
    cp "$CARGO_TARGET_DIR-tool/bench-release/shale" "$TMP/bin/tool"
  fi
}

# wait_quiet: with --max-load, wait for a quiet computer; the load goes into the log
wait_quiet() {
  local waited=0 load
  load=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
  if [ -n "$MAXLOAD" ]; then
    while [ "$waited" -lt "$MAXWAIT" ] && ! awk -v l="$load" -v m="$MAXLOAD" 'BEGIN { exit !(l < m) }'; do
      echo "   load average $load is not below $MAXLOAD: waiting"
      sleep 15
      waited=$((waited + 15))
      load=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
    done
  fi
  echo "   load average $load before the run"
}

# run_one <name> <round>: one run of one side
run_one() {
  local name=$1 round=$2 rev full args
  rev=$X
  [ "$name" = x ] || rev=$Y
  eval "full=\$FULL_$name"
  wait_quiet
  echo "== run $round of $RUNS: $name ($(git rev-parse --short "$full"))"
  args=(--bench "$OUT/runs/$name.$round.json" --profile "$PROFILE")
  [ -z "$FILTER" ] || args+=(--filter "$FILTER")
  (cd "$TMP/$name" \
    && SHALE_BENCH_COMMIT=$full SHALE_BENCH_DESCRIBE=$(git describe --tags --always "$full") SHALE_BENCH_PROFILE=bench-release \
       "$TMP/bin/$name" "${args[@]}") || fail "the run of $rev failed"
}

prepare x "$X"
prepare y "$Y"
prepare_tool

# X, Y, Y, X, X, Y ...: a busy minute hits both sides alike, and neither always runs first
for round in $(seq 1 "$RUNS"); do
  if [ $((round % 2)) = 1 ]; then
    run_one x "$round"
    run_one y "$round"
  else
    run_one y "$round"
    run_one x "$round"
  fi
done
for name in x y; do
  "$TMP/bin/tool" --bench-merge "$OUT/$name.json" "$OUT"/runs/$name.*.json || fail "the runs of $name could not be merged"
done

cargs=(--threshold "$TIME_PCT" --bytes-threshold "$BYTES_PCT" --allocs-threshold "$ALLOCS_PCT" --out "$OUT")
[ -z "$CHANGED" ] || cargs+=(--changed "$CHANGED")
"$TMP/bin/tool" --bench-compare "$OUT/x.json" "$OUT/y.json" "${cargs[@]}"
code=$?

# flamegraphs of what the comparison flagged: X and Y of each, and the difference
if [ "$FLAMES" = yes ] && [ "$code" != 2 ]; then
  if ! command -v jq >/dev/null; then
    echo "bench-compare.sh: jq is missing: no flamegraphs" >&2
  else
    ids=$(jq -r '[.regressions[].id, .noisy[], .improvements[].id] | reduce .[] as $i ([]; if index($i) then . else . + [$i] end) | .[]' "$OUT/compare.json" | head -n "$FLAMEMAX")
    for id in $ids; do
      for name in x y; do
        eval "full=\$FULL_$name"
        SHALE_BENCH_COMMIT=$full SHALE_BENCH_DESCRIBE="$name $(git describe --tags --always "$full")" \
          bash "$ROOT/scripts/bench.sh" --flamegraph-only --no-build --bin "$TMP/bin/$name" --profile "$PROFILE" --filter "=$id" \
          --flame-dir "$OUT/flamegraphs/$name" --keep-folded || true
      done
      if command -v inferno-diff-folded >/dev/null && [ -s "$OUT/flamegraphs/x/$id.folded" ] && [ -s "$OUT/flamegraphs/y/$id.folded" ]; then
        mkdir -p "$OUT/flamegraphs/diff"
        inferno-diff-folded "$OUT/flamegraphs/x/$id.folded" "$OUT/flamegraphs/y/$id.folded" 2>/dev/null \
          | inferno-flamegraph --title "$id: X -> Y (red: more in Y)" > "$OUT/flamegraphs/diff/$id.svg" 2>/dev/null || true
      fi
    done
    echo "flamegraphs: $OUT/flamegraphs/{x,y,diff}"
  fi
fi
echo "report: $OUT/compare.md and $OUT/compare.json (verdict: $(jq -r .verdict "$OUT/compare.json" 2>/dev/null || echo "see compare.json"), exit code $code)"
exit $code

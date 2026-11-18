#!/bin/bash
# Benchmarks two commits and compares them (BUILD-030, raba/shale#343). For an agent or a release.
#
#   scripts/bench-compare.sh <commit-X> <commit-Y> [--profile quick|full|worst]
#                            [--threshold PCT] [--bytes-threshold PCT] [--out DIR]
#
# X is the older commit, Y the newer one. Each gets a temporary git worktree (removed at the end),
# is built with `--profile bench-release --features bench` and run by its own binary; a commit
# without the benchmark harness (no `bench` feature) is an error. Y's binary then compares the two
# result files: DIR (default target/bench-compare-out) gets x.json, y.json, compare.md and
# compare.json (docs/perf/README.md describes them). Exit code 0: no regression above the
# thresholds (default: median time 10 %, peak bytes 15 %); 1: at least one regression, listed
# on stdout; 2: an error (a commit without the harness, a build or run that failed).
# CARGO_TARGET_DIR (default target/bench-compare) is shared by both builds.
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2

X="" Y="" PROFILE=full TIME_PCT=10 BYTES_PCT=15 OUT=target/bench-compare-out
while [ $# -gt 0 ]; do
  case $1 in
    --profile) shift; PROFILE=${1:-} ;;
    --threshold) shift; TIME_PCT=${1:-} ;;
    --bytes-threshold) shift; BYTES_PCT=${1:-} ;;
    --out) shift; OUT=${1:-} ;;
    -h|--help) sed -n '2,/^set -uo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "bench-compare.sh: unknown option $1" >&2; exit 2 ;;
    *) if [ -z "$X" ]; then X=$1; elif [ -z "$Y" ]; then Y=$1; else echo "bench-compare.sh: two commits only" >&2; exit 2; fi ;;
  esac
  shift
done
if [ -z "$Y" ]; then
  echo "usage: scripts/bench-compare.sh <commit-X> <commit-Y> [--profile P] [--threshold PCT] [--bytes-threshold PCT] [--out DIR]" >&2
  exit 2
fi
case $PROFILE in quick|full|worst) ;; *) echo "bench-compare.sh: --profile is quick, full or worst, not $PROFILE" >&2; exit 2 ;; esac

TMP=$(mktemp -d)
cleanup() {
  for w in "$TMP"/x "$TMP"/y; do
    [ -e "$w" ] && git worktree remove --force "$w" >/dev/null 2>&1
  done
  git worktree prune
  rm -rf "$TMP"
}
trap cleanup EXIT
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target/bench-compare}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

# bench_commit <name x|y> <rev>: the worktree, the build and the run; the result is $OUT/<name>.json
bench_commit() {
  local name=$1 rev=$2 full
  full=$(git rev-parse --verify -q "$rev^{commit}") || { echo "bench-compare.sh: $rev is not a commit" >&2; return 2; }
  git worktree add -q --detach "$TMP/$name" "$full" || return 2
  if ! git -C "$TMP/$name" show "HEAD:Cargo.toml" | command grep -q '^bench = ' || [ ! -d "$TMP/$name/src/bench" ]; then
    echo "bench-compare.sh: commit $rev has no benchmark harness (no feature \`bench\`; it came with raba/shale#343), so it cannot be measured" >&2
    return 2
  fi
  echo "== building $rev ($(git rev-parse --short "$full"))"
  (cd "$TMP/$name" && cargo build --profile bench-release --features bench --locked) || { echo "bench-compare.sh: the build of $rev failed" >&2; return 2; }
  echo "== running $rev"
  (cd "$TMP/$name" \
    && SHALE_BENCH_COMMIT=$full SHALE_BENCH_DESCRIBE=$(git describe --tags --always "$full") SHALE_BENCH_PROFILE=bench-release \
       "$CARGO_TARGET_DIR/bench-release/shale" --bench "$OUT/$name.json" --profile "$PROFILE") || { echo "bench-compare.sh: the run of $rev failed" >&2; return 2; }
}

bench_commit x "$X" || exit 2
# X's binary is replaced by Y's build: keep Y's for the comparison
bench_commit y "$Y" || exit 2
"$CARGO_TARGET_DIR/bench-release/shale" --bench-compare "$OUT/x.json" "$OUT/y.json" \
  --threshold "$TIME_PCT" --bytes-threshold "$BYTES_PCT" --out "$OUT"
code=$?
echo "report: $OUT/compare.md and $OUT/compare.json"
exit $code

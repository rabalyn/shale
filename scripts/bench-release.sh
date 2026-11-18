#!/bin/bash
# The performance step of a release (BUILD-034, raba/shale#343), run by scripts/make-release.sh
# in the release worktree before the release commit:
#
#   scripts/bench-release.sh <worktree> <X.Y.Z> [--allow-perf-regression]
#
# Builds the benchmark binary of the worktree (`--profile bench-release --features bench`), runs
# the `full` profile and the `worst` profile and stores the results as docs/perf/results/vX.Y.Z.json
# and docs/perf/results-worst/vX.Y.Z.json. Against the previous release's stored file (the highest
# version below X.Y.Z; none: nothing is compared) it writes docs/perf/compare/vX.Y.Z.{md,json}
# and docs/perf/compare/vX.Y.Z-worst.{md,json} (`shale --bench-compare`, thresholds 10 % median
# time and 15 % peak bytes), and makes docs/perf/index.html. All of it goes into the release
# commit. A regression stops the release (exit 1, the regressions on stderr) unless
# --allow-perf-regression is given (then they are printed and the release goes on); any other
# failure ends with exit 2. Environment, for scripts/test-release.sh: BENCH_BIN (a program used
# instead of building and running the worktree's), BENCH_TARGET_DIR (the build folder,
# default <git dir>/release/bench-target).
set -uo pipefail

ALLOW=no
POS=()
for a in "$@"; do
  case $a in
    --allow-perf-regression) ALLOW=yes ;;
    -h|--help) sed -n '2,/^set -uo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "bench-release.sh: unknown option $a" >&2; exit 2 ;;
    *) POS+=("$a") ;;
  esac
done
if [ ${#POS[@]} -ne 2 ]; then
  echo "usage: scripts/bench-release.sh <worktree> <X.Y.Z> [--allow-perf-regression]" >&2
  exit 2
fi
WT=${POS[0]}
V=${POS[1]}
[ -d "$WT" ] || { echo "bench-release.sh: $WT is not a folder" >&2; exit 2; }
cd "$WT" || exit 2

. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/bench-env.sh"
if [ -n "${BENCH_BIN:-}" ]; then
  BIN=$BENCH_BIN
else
  COMMON=$(git rev-parse --path-format=absolute --git-common-dir)
  export CARGO_TARGET_DIR=${BENCH_TARGET_DIR:-$COMMON/release/bench-target}
  echo "== building the benchmark binary"
  cargo build --profile bench-release --features bench --locked >&2 || { echo "bench-release.sh: the benchmark build failed" >&2; exit 2; }
  BIN=$CARGO_TARGET_DIR/bench-release/shale
fi

export SHALE_BENCH_COMMIT=$(git rev-parse HEAD)
export SHALE_BENCH_DESCRIBE="v$V"
export SHALE_BENCH_PROFILE=bench-release

# the highest stored result below the version of $2 (a folder of vX.Y.Z.json files), or nothing
previous() {
  local dir=$1 f n
  for f in "$dir"/v*.json; do
    [ -e "$f" ] || continue
    n=${f##*/v}
    n=${n%.json}
    [[ $n =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
    [ "$n" = "$V" ] && continue
    printf '%s\n' "$n"
  done | sort -t. -k1,1n -k2,2n -k3,3n | awk -v v="$V" 'function key(s, a) { split(s, a, "."); return a[1] * 1e12 + a[2] * 1e6 + a[3] } key($0) < key(v) { last = $0 } END { if (last != "") print last }'
}

REGRESSED=""
# run_set <profile> <results folder> <compare name suffix>
run_set() {
  local profile=$1 results=$2 suffix=$3 prev code
  mkdir -p "$results" docs/perf/compare
  prev=$(previous "$results")
  echo "== benchmark: the $profile profile" >&2
  "$BIN" --bench "$results/v$V.json" --profile "$profile" >&2 || { echo "bench-release.sh: the $profile run failed" >&2; exit 2; }
  if [ -z "$prev" ]; then
    echo "no earlier release result in $results: nothing to compare the $profile run with" >&2
    return 0
  fi
  "$BIN" --bench-compare "$results/v$prev.json" "$results/v$V.json" --out "docs/perf/compare/.tmp-$profile"
  code=$?
  case $code in
    0|1) ;;
    *) echo "bench-release.sh: the comparison of the $profile run failed" >&2; exit 2 ;;
  esac
  cp "docs/perf/compare/.tmp-$profile/compare.md" "docs/perf/compare/v$V$suffix.md"
  cp "docs/perf/compare/.tmp-$profile/compare.json" "docs/perf/compare/v$V$suffix.json"
  rm -rf "docs/perf/compare/.tmp-$profile"
  if [ "$code" = 1 ]; then
    REGRESSED="$REGRESSED $profile(v$prev -> v$V: docs/perf/compare/v$V$suffix.md)"
  fi
}

run_set full docs/perf/results ""
run_set worst docs/perf/results-worst "-worst"
"$BIN" --bench-report docs/perf docs/perf/index.html >&2 || { echo "bench-release.sh: the report could not be made" >&2; exit 2; }

if [ -n "$REGRESSED" ]; then
  if [ "$ALLOW" = yes ]; then
    echo "performance regressions, allowed by --allow-perf-regression:$REGRESSED" >&2
    exit 0
  fi
  echo "performance regression:$REGRESSED" >&2
  echo "Fix it, or run the release again with --allow-perf-regression (say why in the release issue)." >&2
  exit 1
fi
exit 0

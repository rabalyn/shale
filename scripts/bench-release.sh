#!/bin/bash
# The performance step of a release (BUILD-034, raba/shale#343), run by scripts/make-release.sh
# in the release worktree before the release commit:
#
#   scripts/bench-release.sh <worktree> <X.Y.Z> [--allow-perf-regression] [--note FILE]
#
# Builds the benchmark binary of the worktree (`--profile bench-release --features bench`), runs
# the `full` profile (BENCH_RUNS times, default 2, merged with `--bench-merge`: the best median of
# the runs and their spread, BUILD-036) and the `worst` profile (BENCH_WORST_RUNS times, default 1)
# and stores the results as docs/perf/results/vX.Y.Z.json
# and docs/perf/results-worst/vX.Y.Z.json. Against the previous release's stored file (the highest
# version below X.Y.Z; none: nothing is compared) it writes docs/perf/compare/vX.Y.Z.{md,json}
# and docs/perf/compare/vX.Y.Z-worst.{md,json} (`shale --bench-compare`, thresholds 10 % median
# time and 15 % peak bytes), and makes docs/perf/index.html. All of it goes into the release
# commit.
#
# A regression stops the release (exit 1, the regressions on stderr, how to follow it up: the
# agent playbook in docs/perf/README.md) unless --allow-perf-regression is given (then they are
# printed and the release goes on); any other failure ends with exit 2. The stored comparison of a
# release is one run of each side, on whatever else the computer was doing, so a flagged TIME
# regression is confirmed before it counts (BUILD-034, #408): the flagged scenarios alone are
# measured again against the previous release, X and Y interleaved, BENCH_RECHECK_RUNS times
# (default 5) by scripts/bench-compare.sh (`--filter`), and the release stops only when the
# regression shows again there. Allocations and peak bytes are exact numbers and need no
# confirmation. The re-run is kept in docs/perf/compare/vX.Y.Z[-worst]-recheck.{md,json}, whatever
# it found (the first comparison stays as it was, for the history); --note FILE gets a summary
# of the numbers in Markdown, for the release issue (scripts/release-lib.sh posts it).
# Environment: BENCH_RECHECK_RUNS, BENCH_MAX_LOAD (passed to bench-compare.sh as --max-load).
# For scripts/test-release.sh: BENCH_BIN (a program used instead of building and running the
# worktree's), BENCH_COMPARE (instead of scripts/bench-compare.sh), BENCH_TARGET_DIR (the build
# folder, default <git dir>/release/bench-target).
set -uo pipefail

ALLOW=no
NOTE=""
POS=()
while [ $# -gt 0 ]; do
  case $1 in
    --allow-perf-regression) ALLOW=yes ;;
    --note) [ -n "${2:-}" ] || { echo "bench-release.sh: --note needs a file" >&2; exit 2; }; shift; NOTE=$1 ;;
    -h|--help) sed -n '2,/^set -uo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "bench-release.sh: unknown option $1" >&2; exit 2 ;;
    *) POS+=("$1") ;;
  esac
  shift
done
if [ ${#POS[@]} -ne 2 ]; then
  echo "usage: scripts/bench-release.sh <worktree> <X.Y.Z> [--allow-perf-regression] [--note FILE]" >&2
  exit 2
fi
case $NOTE in ''|/*) ;; *) NOTE=$PWD/$NOTE ;; esac
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
WT=${POS[0]}
V=${POS[1]}
[ -d "$WT" ] || { echo "bench-release.sh: $WT is not a folder" >&2; exit 2; }
cd "$WT" || exit 2

ORIG_RUSTFLAGS=${RUSTFLAGS:-}
. "$SCRIPT_DIR/bench-env.sh"
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
CONFIRMED=""
RECHECK_RUNS=${BENCH_RECHECK_RUNS:-5}
case $RECHECK_RUNS in ''|*[!0-9]*|0) echo "bench-release.sh: BENCH_RECHECK_RUNS needs a number of at least 1" >&2; exit 2 ;; esac
RUNS_DIR=$(mktemp -d)
trap 'rm -rf "$RUNS_DIR"' EXIT
case ${BENCH_RUNS:-2} in ''|*[!0-9]*|0) echo "bench-release.sh: BENCH_RUNS needs a number of at least 1" >&2; exit 2 ;; esac
case ${BENCH_WORST_RUNS:-1} in ''|*[!0-9]*|0) echo "bench-release.sh: BENCH_WORST_RUNS needs a number of at least 1" >&2; exit 2 ;; esac
# run_set <profile> <results folder> <compare name suffix>
run_set() {
  local profile=$1 results=$2 suffix=$3 runs=$4 prev code i
  local parts=()
  mkdir -p "$results" docs/perf/compare
  prev=$(previous "$results")
  for i in $(seq 1 "$runs"); do
    echo "== benchmark: the $profile profile, run $i of $runs" >&2
    "$BIN" --bench "$RUNS_DIR/$profile.$i.json" --profile "$profile" >&2 || { echo "bench-release.sh: the $profile run failed" >&2; exit 2; }
    parts+=("$RUNS_DIR/$profile.$i.json")
  done
  "$BIN" --bench-merge "$results/v$V.json" "${parts[@]}" >&2 || { echo "bench-release.sh: the $profile runs could not be merged" >&2; exit 2; }
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
    confirm "$profile" "$suffix" "$prev"
  fi
}

# the lines `  <id> <metric> <x> -> <y> (<delta> %, confidence <c>)` of the regressions of a compare.json
# that pass the jq filter $2 ('true' for all)
regression_lines() {
  jq -r ".regressions[] | select($2) | \"  \(.id) \(.metric) \(.x) -> \(.y) (\(.delta_pct * 10 | round / 10) %, confidence \(.confidence))\"" "$1"
}

# recheck <profile> <suffix> <prev> <ids>: the flagged scenarios again, alone, against the previous release,
# interleaved (scripts/bench-compare.sh); the result is docs/perf/compare/v$V$suffix-recheck.{md,json}
recheck() {
  local profile=$1 suffix=$2 prev=$3 ids=$4 xc filter out args
  xc=$(jq -r '.x.commit // empty' "docs/perf/compare/v$V$suffix.json")
  git rev-parse --verify -q "$xc^{commit}" >/dev/null 2>&1 || xc="v$prev"
  filter=$(printf '%s\n' "$ids" | sed 's/^/=/' | paste -sd, -)
  out="docs/perf/compare/.tmp-recheck-$profile"
  args=("$xc" "$(git rev-parse HEAD)" --profile "$profile" --runs "$RECHECK_RUNS" --filter "$filter" --out "$out")
  [ -z "${BENCH_MAX_LOAD:-}" ] || args+=(--max-load "$BENCH_MAX_LOAD")
  echo "== confirming the time regression of $(printf '%s' "$ids" | tr '\n' ' ')against v$prev: $RECHECK_RUNS interleaved runs of each side" >&2
  # the build flags of this script are already in RUSTFLAGS: bench-compare.sh adds them to what it is given, not to them twice
  env RUSTFLAGS="$ORIG_RUSTFLAGS" bash "${BENCH_COMPARE:-$SCRIPT_DIR/bench-compare.sh}" "${args[@]}" >&2
  if [ ! -f "$out/compare.json" ] || [ "$(jq -r '.verdict // "error"' "$out/compare.json")" = error ]; then
    echo "bench-release.sh: the re-run of the flagged $profile scenarios failed: $(jq -r '.error // "no compare.json"' "$out/compare.json" 2>/dev/null)" >&2
    exit 2
  fi
  cp "$out/compare.md" "docs/perf/compare/v$V$suffix-recheck.md"
  cp "$out/compare.json" "docs/perf/compare/v$V$suffix-recheck.json"
  rm -rf "$out"
}

# note_section <profile> <suffix> <prev> <ids> <confirmed ids>: the numbers of the re-run, for the release issue
note_section() {
  local profile=$1 suffix=$2 prev=$3 ids=$4 confirmed=$5 first re
  first="docs/perf/compare/v$V$suffix.json"
  re="docs/perf/compare/v$V$suffix-recheck.json"
  [ -n "$NOTE" ] || return 0
  {
    printf '#### %s profile: v%s -> v%s\n\n' "$profile" "$prev" "$V"
    jq -r --arg ids "$ids" --arg confirmed "$confirmed" --slurpfile first "$first" '
      def ms: (. / 1e4 | round / 100);
      "The stored comparison (one run of each side) flagged a time regression in \(($ids | split("\n") | map(select(. != "")) | length)) scenario(s). They were measured again alone against `\(.x.describe)` (`\(.x.commit[0:10])`), X and Y interleaved, \(.y.repeats) runs of each side, load average \(.x.load) (X) and \(.y.load) (Y).\n",
      "| Scenario | First run | Re-run X (ms) | Re-run Y (ms) | Re-run | Verdict |",
      "|---|---:|---:|---:|---:|---|",
      (.rows[] | select(.id as $i | $ids | split("\n") | index($i)) | . as $r
        | ($first[0].regressions | map(select(.id == $r.id and .metric == "time")) | .[0].delta_pct // null) as $f
        | "| `\($r.id)` | \(if $f == null then "-" else "+\($f * 10 | round / 10) %" end) | \($r.x_median_ns | ms) | \($r.y_median_ns | ms) | \(($r.time_delta_pct * 10 | round / 10)) % | \($r.time_verdict) |"),
      "",
      (if $confirmed == "" then "**Not confirmed**: the regression did not show again in the interleaved runs; it was noise." else "**Confirmed**: \($confirmed | split("\n") | map(select(. != "")) | join(", "))." end)
    ' "$re"
    printf '\nFiles: docs/perf/compare/v%s%s-recheck.md and .json\n\n' "$V" "$suffix"
  } >> "$NOTE"
}

# confirm <profile> <suffix> <prev>: the first comparison said regression. Allocations and peak bytes are exact:
# they stand. A time regression is measured again (recheck) and stands only when the re-run shows it too.
confirm() {
  local profile=$1 suffix=$2 prev=$3 first ids re confirmed="" lines=""
  first="docs/perf/compare/v$V$suffix.json"
  if ! command -v jq >/dev/null; then
    echo "bench-release.sh: jq is missing, so a time regression cannot be confirmed by a re-run: it counts as it is" >&2
    REGRESSED="$REGRESSED $profile(v$prev -> v$V: docs/perf/compare/v$V$suffix.md)"
    return 0
  fi
  lines=$(regression_lines "$first" '.metric != "time"')
  ids=$(jq -r '[.regressions[] | select(.metric == "time") | .id] | unique | .[]' "$first")
  if [ -n "$ids" ]; then
    recheck "$profile" "$suffix" "$prev" "$ids"
    re="docs/perf/compare/v$V$suffix-recheck.json"
    confirmed=$(jq -r '[.regressions[] | select(.metric == "time") | .id] | unique | .[]' "$re")
    note_section "$profile" "$suffix" "$prev" "$ids" "$confirmed" "$re"
    if [ -n "$confirmed" ]; then
      lines="$lines${lines:+
}$(regression_lines "$re" '.metric == "time"')"
    else
      echo "the time regression of $(printf '%s' "$ids" | tr '\n' ' ')in the $profile profile was not confirmed by the re-run: noise" >&2
    fi
  fi
  if [ -n "$lines" ] || [ -z "$ids" ]; then
    REGRESSED="$REGRESSED $profile(v$prev -> v$V: docs/perf/compare/v$V$suffix.md)"
    CONFIRMED="$CONFIRMED${lines:+$lines
}"
  fi
}

run_set full docs/perf/results "" "${BENCH_RUNS:-2}"
run_set worst docs/perf/results-worst "-worst" "${BENCH_WORST_RUNS:-1}"
"$BIN" --bench-report docs/perf docs/perf/index.html >&2 || { echo "bench-release.sh: the report could not be made" >&2; exit 2; }

if [ -n "$REGRESSED" ]; then
  if [ "$ALLOW" = yes ]; then
    echo "performance regressions, allowed by --allow-perf-regression:$REGRESSED" >&2
    exit 0
  fi
  echo "performance regression:$REGRESSED" >&2
  printf '%s' "$CONFIRMED" >&2
  echo "Follow it up as the agent playbook in docs/perf/README.md says: flamegraphs of both versions (the flagged scenarios were measured again already), an issue with the numbers." >&2
  echo "Fix it, or run the release again with --allow-perf-regression (say why in the release issue)." >&2
  exit 1
fi
exit 0

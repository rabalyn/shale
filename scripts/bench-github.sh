#!/bin/bash
# The benchmark statistics of a release build on GitHub (BUILD-041, raba/shale#408), run by a job of
# .github/workflows/build.yml on each system after the release page is up:
#
#   scripts/bench-github.sh <tag> <system> [--flamegraphs] [--out DIR]
#
# <system> is linux-x86_64, macos-arm64 or windows-x86_64. Builds the benchmark binary
# (`--profile bench-release --features bench`), runs the `full` profile once and writes
# DIR/shale-bench-<version>-<system>.json (DIR default bench-out; the workflow keeps it as an asset
# of the release). Then it looks through the last releases for the previous result of the *same
# system* (an asset `shale-bench-*-<system>.json`; the first that has one), compares with it
# (`shale --bench-compare`) and writes what is odd (regressions, noisy and changed scenarios, the
# best improvements) into the job summary ($GITHUB_STEP_SUMMARY, else standard output). A runner
# of a public service is shared and its hardware changes between runs: these are statistics, not a
# gate (the gate is the owner's workstation, scripts/bench-release.sh), so a comparison that says
# `regression` is a line in the summary and nothing else. The exit code is 0 unless the
# measurement itself did not run (1); the workflow's steps have `continue-on-error`.
#
# --flamegraphs (the Linux job; needs perf and inferno, BUILD-033) also makes the flamegraphs of
# the scenarios the comparison flagged (regressions, noisy ones, the best improvements) and then
# the slowest ones, up to FLAME_MAX (default 8), with scripts/bench.sh, and packs them as
# DIR/shale-flamegraphs-<version>.tar.gz (a folder `flamegraphs/` of <id>.svg), which the workflow
# attaches to the release (#400): they are not kept in git. The page docs/perf/index.html links
# to that attachment. Without perf or inferno there is no file and no failure.
#
# Environment: GITHUB_REPOSITORY (default rabalyn/shale), GITHUB_SHA, GITHUB_OUTPUT (gets
# `result=<file>`, `compared=<tag or empty>` and `flamegraphs=<file>`), FLAME_MAX. For tests:
# BENCH_BIN (a program instead of building and running the worktree's), BENCH_GH (instead of gh),
# BENCH_FLAMES (instead of scripts/bench.sh).
set -uo pipefail

OUT=bench-out
FLAMES=no
POS=()
while [ $# -gt 0 ]; do
  case $1 in
    --flamegraphs) FLAMES=yes ;;
    --out) [ -n "${2:-}" ] || { echo "bench-github.sh: --out needs a folder" >&2; exit 2; }; shift; OUT=$1 ;;
    -h|--help) sed -n '2,/^set -uo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "bench-github.sh: unknown option $1" >&2; exit 2 ;;
    *) POS+=("$1") ;;
  esac
  shift
done
if [ ${#POS[@]} -ne 2 ]; then
  echo "usage: scripts/bench-github.sh <tag> <system: linux-x86_64|macos-arm64|windows-x86_64> [--flamegraphs] [--out DIR]" >&2
  exit 2
fi
TAG=${POS[0]}
SYSTEM=${POS[1]}
VERSION=${TAG#v}
case $SYSTEM in linux-x86_64|macos-arm64|windows-x86_64) ;; *) echo "bench-github.sh: the system is linux-x86_64, macos-arm64 or windows-x86_64, not $SYSTEM" >&2; exit 2 ;; esac
GH=${BENCH_GH:-gh}
REPO=${GITHUB_REPOSITORY:-rabalyn/shale}
NAME="shale-bench-$VERSION-$SYSTEM.json"
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT" || exit 2
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

summary() { cat >> "${GITHUB_STEP_SUMMARY:-/dev/stdout}"; }
output() { [ -z "${GITHUB_OUTPUT:-}" ] || echo "$1=$2" >> "$GITHUB_OUTPUT"; }

if [ -n "${BENCH_BIN:-}" ]; then
  BIN=$BENCH_BIN
else
  . "$ROOT/scripts/bench-env.sh"
  echo "== building the benchmark binary"
  cargo build --profile bench-release --features bench --locked || { printf '## Benchmarks (%s): the build failed\n' "$SYSTEM" | summary; exit 1; }
  BIN=target/bench-release/shale
  [ -x "$BIN" ] || BIN=target/bench-release/shale.exe
fi

echo "== benchmark: the full profile on $SYSTEM"
SHALE_BENCH_COMMIT=${GITHUB_SHA:-$(git rev-parse HEAD)} SHALE_BENCH_DESCRIBE=$TAG SHALE_BENCH_PROFILE=bench-release \
  "$BIN" --bench "$OUT/$NAME" --profile full || { printf '## Benchmarks (%s): the run failed\n\nNo result for %s.\n' "$SYSTEM" "$TAG" | summary; exit 1; }
output result "$OUT/$NAME"

# the previous result of the same system: the newest of the last releases that has an asset for it
PREV_TAG=""
rm -rf "$OUT/previous"
tags=$("$GH" release list -R "$REPO" --limit 10 --exclude-drafts --exclude-pre-releases --json tagName -q '.[].tagName' 2>/dev/null) || tags=""
for t in $tags; do
  [ "$t" = "$TAG" ] && continue
  mkdir -p "$OUT/previous"
  if "$GH" release download "$t" -R "$REPO" --pattern "shale-bench-*-$SYSTEM.json" --dir "$OUT/previous" >/dev/null 2>&1 \
    && [ -n "$(ls "$OUT/previous"/*.json 2>/dev/null)" ]; then
    PREV_TAG=$t
    break
  fi
  rm -rf "$OUT/previous"
done
output compared "$PREV_TAG"

{
  printf '## Benchmarks: %s, %s (full profile)\n\n' "$SYSTEM" "$TAG"
  printf 'Statistics only: a shared runner is noisy and its hardware changes from run to run, so nothing here fails the release (the gate is the owner'"'"'s workstation, scripts/bench-release.sh). The result is kept as the release asset %s.\n\n' "$NAME"
} | summary

compare_and_summarise() {
  if [ -z "$PREV_TAG" ]; then
    printf 'No earlier release has a result for %s: nothing to compare with.\n' "$SYSTEM" | summary
    return 0
  fi
  for prev in "$OUT/previous"/*.json; do break; done
  "$BIN" --bench-compare "$prev" "$OUT/$NAME" --out "$OUT/compare"
  code=$?
  if [ "$code" -gt 1 ] || [ ! -f "$OUT/compare/compare.json" ]; then
    printf 'The comparison with %s failed (exit code %s): %s\n' "$PREV_TAG" "$code" "$(jq -r '.error // empty' "$OUT/compare/compare.json" 2>/dev/null)" | summary
    return 0
  fi
  if ! command -v jq >/dev/null; then
    { printf 'Compared with %s (same system); jq is missing, the top of the comparison:\n\n' "$PREV_TAG"; head -n 40 "$OUT/compare/compare.md"; } | summary
    return 0
  fi
  jq -r --arg prev "$PREV_TAG" '
    def ms: (. / 1e4 | round / 100);
    def val($m): if $m == "time" then "\(. | ms) ms" elif $m == "peak_bytes" then "\(. / 1024 | round) KiB" else "\(.)" end;
    def row: . as $r | "| `\($r.id)` | \($r.metric) | \($r.x | val($r.metric)) | \($r.y | val($r.metric)) | \($r.delta_pct * 10 | round / 10) % | \($r.confidence) |";
    "Compared with `\($prev)` on the same system: **\(.verdict)**; \(.summary.compared) scenarios compared, \(.summary.faster) faster, \(.summary.slower) slower, \(.summary.regressions) over the threshold, \(.summary.noisy) noisy, \(.summary.changed) changed (load average \(.x.load) before, \(.y.load) now).\n",
    (if (.regressions | length) > 0 then
      "### Over the threshold\n\n| Scenario | Metric | Before | Now | Change | Confidence |\n|---|---|---:|---:|---:|---|\n" + (.regressions | map(row) | join("\n")) + "\n"
     else empty end),
    (if (.noisy | length) > 0 then "### Noisy (moved over the threshold, inside the spread of the runs)\n\n" + (.noisy | map("`\(.)`") | join(", ")) + "\n" else empty end),
    (if (.improvements | length) > 0 then
      "### Best improvements\n\n| Scenario | Metric | Before | Now | Change | Confidence |\n|---|---|---:|---:|---:|---|\n" + (.improvements[:5] | map(row) | join("\n")) + "\n"
     else empty end),
    (if (.regressions | length) + (.noisy | length) == 0 then "Nothing odd.\n" else empty end)
  ' "$OUT/compare/compare.json" | summary
}

# flamegraphs: the scenarios the comparison flagged first, then the slowest ones, made by scripts/bench.sh
# from the binary that measured, packed into $OUT/shale-flamegraphs-<version>.tar.gz
flamegraphs() {
  local max=${FLAME_MAX:-8} ids="" slowest tarball="shale-flamegraphs-$VERSION.tar.gz" n
  command -v jq >/dev/null || { echo "bench-github.sh: jq is missing: no flamegraphs" >&2; return 0; }
  if [ -f "$OUT/compare/compare.json" ]; then
    ids=$(jq -r '[.regressions[].id, .noisy[], .improvements[].id] | .[]' "$OUT/compare/compare.json" 2>/dev/null)
  fi
  slowest=$(jq -r '.scenarios | sort_by(-.median_ns) | .[].id' "$OUT/$NAME")
  ids=$(printf '%s\n%s\n' "$ids" "$slowest" | awk 'NF && !seen[$0]++' | head -n "$max")
  [ -n "$ids" ] || return 0
  rm -rf "$OUT/flamegraphs"
  bash "${BENCH_FLAMES:-$ROOT/scripts/bench.sh}" --flamegraph-only --no-build --bin "$BIN" --profile full \
    --filter "$(printf '%s\n' "$ids" | sed 's/^/=/' | paste -sd, -)" --flame-dir "$OUT/flamegraphs" \
    || echo "bench-github.sh: the flamegraphs could not be made" >&2
  n=$(find "$OUT/flamegraphs" -name '*.svg' 2>/dev/null | wc -l)
  if [ "$n" -eq 0 ]; then
    printf '\nNo flamegraphs were made (perf or inferno is missing, or perf may not profile here).\n' | summary
    return 0
  fi
  tar -C "$OUT" -czf "$OUT/$tarball" flamegraphs || return 0
  output flamegraphs "$OUT/$tarball"
  printf '\n%s flamegraphs (%s) are attached to the release as %s.\n' "$n" "$(printf '%s' "$ids" | tr '\n' ' ')" "$tarball" | summary
}

compare_and_summarise
[ "$FLAMES" != yes ] || flamegraphs
exit 0

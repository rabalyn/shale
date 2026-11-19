#!/bin/bash
# Tests the retry of `.forgejo/scripts/env-tests.sh tex-suite` (spec DEV-011, #327): a fake `cargo`
# prints what each run of the tex_env suite says, a fake `tex-env.sh` records what is asked of it.
#   bash scripts/test-env-tests.sh
set -u
SCRIPT="$(cd "$(dirname "$0")/.." && pwd)/.forgejo/scripts/env-tests.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0

mkdir -p "$TMP/bin"
cat > "$TMP/bin/cargo" <<'CARGO'
#!/bin/sh
# the output of run n is out.n of the scenario folder
n=$(cat "$SCENARIO/cargo.count" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$SCENARIO/cargo.count"
cat "$SCENARIO/out.$n"
CARGO
cat > "$TMP/bin/tex-env.sh" <<'TEXENV'
#!/bin/sh
echo "$*" >> "$SCENARIO/texenv.calls"
case "$1" in
  build) [ ! -f "$SCENARIO/build.fails" ] ;;
  status) echo "image shale-tex-env: there (TeX Live 2026 from the fake mirror)" ;;
esac
TEXENV
chmod +x "$TMP/bin/cargo" "$TMP/bin/tex-env.sh"

OK='test result: ok. 12 passed; 0 failed; 0 ignored'
OTHER='test tex_env_x ... FAILED
! Undefined control sequence.
test result: FAILED. 11 passed; 1 failed; 0 ignored'
FONT='test tex_env_y ... FAILED
koma-script/scrsize11pt.clo:100: Font \TU/lmr/m/n/10.95=[lmroman10-regular]:+tlig; not loadable: Metric (TFM) file or installed font not found.
test result: FAILED. 11 passed; 1 failed; 0 ignored'
METRIC='test tex_env_z ... FAILED
Package fontspec Error: Metric data not found for lmroman10-regular
test result: FAILED. 11 passed; 1 failed; 0 ignored'

# scenario <name> <exit code> <needle> <cargo runs> <tex-env calls> <first output> [<second output> | build-fails]
scenario() {
  local name=$1 code=$2 needle=$3 runs=$4 calls=$5 first=$6 second=${7:-}
  N=$((N + 1))
  export SCENARIO="$TMP/s$N"
  mkdir -p "$SCENARIO"
  printf '%s\n' "$first" > "$SCENARIO/out.1"
  if [ "$second" = build-fails ]; then touch "$SCENARIO/build.fails"; else printf '%s\n' "$second" > "$SCENARIO/out.2"; fi
  OUT=$(PATH="$TMP/bin:$PATH" SHALE_TEX_ENV_SH="$TMP/bin/tex-env.sh" sh "$SCRIPT" tex-suite 2 2>&1)
  GOT=$?
  local gotruns gotcalls
  gotruns=$(cat "$SCENARIO/cargo.count" 2>/dev/null || echo 0)
  gotcalls=$(paste -sd, "$SCENARIO/texenv.calls" 2>/dev/null)
  if [ "$GOT" = "$code" ] && [ "$gotruns" = "$runs" ] && [ "$gotcalls" = "$calls" ] && printf '%s' "$OUT" | grep -Fq -- "$needle"; then
    echo "ok $N - $name"
  else
    echo "not ok $N - $name (exit $GOT, $gotruns cargo runs, tex-env calls '$gotcalls'): $OUT"
    FAILED=1
  fi
}

scenario "a passing suite runs once and touches no image" 0 "12 passed" 1 "" "$OK"
scenario "a failure that is no font problem is not retried" 1 "no tex_env test passed" 1 "" "$OTHER"
scenario "a font that cannot be loaded says it is the environment, rebuilds on a fresh mirror and the second run passes" 0 "looks like a TeX environment problem, not a code error" 2 "status,build --refresh" "$FONT" "$OK"
scenario "the second run decides: a font failure that stays is a failure" 1 "failed again on the fresh image" 2 "status,build --refresh" "$FONT" "$FONT"
scenario "the metric data wording is a font problem too" 0 "from a fresh mirror" 2 "status,build --refresh" "$METRIC" "$OK"
scenario "an image that cannot be rebuilt is a failure without a second run" 1 "could not be built again" 1 "status,build --refresh" "$FONT" build-fails
scenario "the second run is not retried again, even when it fails differently" 1 "no tex_env test passed" 2 "status,build --refresh" "$FONT" "$OTHER"

exit "$FAILED"

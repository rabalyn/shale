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
echo "${SHALE_TEX_ENV_REPOSITORY:-}" >> "$SCENARIO/texenv.repos"
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

# spec: DEV-025: the mirror variable of the environment reaches the build of the fresh image
SHALE_TEX_ENV_REPOSITORY=https://mirror.example.test/tlnet scenario "dev_025: SHALE_TEX_ENV_REPOSITORY reaches tex-env.sh build" 0 "from a fresh mirror" 2 "status,build --refresh" "$FONT" "$OK"
if [ "$(tail -n 1 "$SCENARIO/texenv.repos")" = "https://mirror.example.test/tlnet" ]; then
  echo "ok - dev_025: the build saw the mirror"
else
  echo "not ok - dev_025: the build did not see SHALE_TEX_ENV_REPOSITORY"
  FAILED=1
fi

# --- which suites a change needs (`suites`, `wanted`; spec DEV-011, FORGE-011, #447) ---
REPO="$TMP/repo"
git init -q "$REPO"
git -C "$REPO" config user.email t@example.test
git -C "$REPO" config user.name t
git -C "$REPO" commit -q --allow-empty -m base
BASE=$(git -C "$REPO" rev-parse HEAD)
# the script works in the repository two folders above it: use a copy inside the test repository
mkdir -p "$REPO/.forgejo/scripts"
cp "$SCRIPT" "$REPO/.forgejo/scripts/env-tests.sh"
git -C "$REPO" add -A
git -C "$REPO" commit -q -m script
BASE=$(git -C "$REPO" rev-parse HEAD)
REPO_SCRIPT="$REPO/.forgejo/scripts/env-tests.sh"
# suites_case <name> <expected suites> <file>…: a commit that touches the files
suites_case() {
  local name=$1 want=$2 f
  shift 2
  git -C "$REPO" reset -q --hard "$BASE"
  for f in "$@"; do mkdir -p "$REPO/$(dirname "$f")"; echo "# x" >> "$REPO/$f"; done
  git -C "$REPO" add -A
  git -C "$REPO" commit -q -m change
  local got wanted exp_wanted=yes
  got=$(cd "$REPO" && sh "$REPO_SCRIPT" suites "$BASE" HEAD)
  wanted=$(cd "$REPO" && sh "$REPO_SCRIPT" wanted "$BASE" HEAD)
  [ -z "$want" ] && exp_wanted=no
  if [ "$got" = "$want" ] && [ "$wanted" = "$exp_wanted" ]; then
    echo "ok - dev_011: suites $name"
  else
    echo "not ok - dev_011: suites $name (got '$got', wanted '$wanted', expected '$want')"
    FAILED=1
  fi
}
ALL="calendar::env_tests mail_env tex_env"
suites_case "CalDAV code" "calendar::env_tests" src/calendar/ui.rs
suites_case "CalDAV environment" "calendar::env_tests" tests/caldav-env/caldav-env.sh
suites_case "mail code (src/mail.rs)" "mail_env" src/mail.rs
suites_case "mail code (src/mail/)" "mail_env" src/mail/imap.rs
suites_case "mail environment" "mail_env" tests/mail-env/mail-env.sh
suites_case "TeX code" "tex_env" src/pdf/tex/build.rs
suites_case "TeX assets" "tex_env" assets/tex/sets/x.txt
suites_case "TeX environment" "tex_env" tests/tex-env/tex-env.sh
suites_case "CalDAV and TeX together, in a fixed order" "calendar::env_tests tex_env" src/pdf/tex/a.rs src/calendar/b.rs
suites_case "CalDAV, mail and TeX together" "$ALL" src/calendar/b.rs src/mail/c.rs tests/tex-env/d
suites_case "the CI workflow runs all three" "$ALL" .forgejo/workflows/ci.yml
suites_case "env-tests.sh itself runs all three" "$ALL" .forgejo/scripts/env-tests.sh
suites_case "another file needs none" "" src/editor/x.rs docs/a.md
suites_case "another CI file needs none" "" .forgejo/scripts/check-coverage.sh

# --- `run` starts only the stacks its suites need (#447) ---
mkdir -p "$TMP/runbin"
cat > "$TMP/runbin/cargo" <<'CARGO'
#!/bin/sh
echo "cargo $*" >> "$SCENARIO/calls"
echo 'test result: ok. 3 passed; 0 failed; 0 ignored'
CARGO
for name in caldav mail tex; do
  cat > "$TMP/runbin/$name-env.sh" <<'ENVSH'
#!/bin/sh
echo "$(basename "$0" -env.sh) $*" >> "$SCENARIO/calls"
ENVSH
done
chmod +x "$TMP/runbin/"*

# run_case <name> <expected calls, comma separated> [VAR=value…]: `run` with those variables
run_case() {
  local name=$1 want=$2 got code
  shift 2
  N=$((N + 1))
  export SCENARIO="$TMP/r$N"
  mkdir -p "$SCENARIO"
  : > "$SCENARIO/calls"
  OUT=$(env -u SHALE_ENV_TESTS_SUITES "$@" PATH="$TMP/runbin:$PATH" SCENARIO="$SCENARIO" \
    SHALE_CALDAV_ENV_SH="$TMP/runbin/caldav-env.sh" SHALE_MAIL_ENV_SH="$TMP/runbin/mail-env.sh" \
    SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
    sh "$SCRIPT" run podman 2>&1)
  code=$?
  got=$(paste -sd, "$SCENARIO/calls")
  if [ "$code" = 0 ] && [ "$got" = "$want" ]; then
    echo "ok - dev_011: run $name"
  else
    echo "not ok - dev_011: run $name (exit $code): got '$got', expected '$want': $OUT"
    FAILED=1
  fi
}
CAL_UP="caldav up"
MAIL_UP="mail up"
CAL_RUN="cargo test --locked -- --ignored calendar::env_tests --test-threads=1"
MAIL_RUN="cargo test --locked -- --ignored mail_env --test-threads=1"
TEX_RUN="cargo test --locked -- --ignored tex_env --test-threads=4"
CAL_DOWN="caldav reset"
MAIL_DOWN="mail reset"
run_case "unset runs all three suites on both stacks and builds the image" \
  "caldav reset,mail reset,$CAL_UP,$MAIL_UP,tex build,$CAL_RUN,$MAIL_RUN,$TEX_RUN,$CAL_DOWN,$MAIL_DOWN"
run_case "a CalDAV list starts only the CalDAV stack, no image" \
  "$CAL_DOWN,$CAL_UP,$CAL_RUN,$CAL_DOWN" "SHALE_ENV_TESTS_SUITES=calendar::env_tests"
run_case "a mail list starts only the mail stack, no image" \
  "$MAIL_DOWN,$MAIL_UP,$MAIL_RUN,$MAIL_DOWN" "SHALE_ENV_TESTS_SUITES=mail_env"
run_case "a TeX list starts no stack and builds the image" \
  "tex build,$TEX_RUN" "SHALE_ENV_TESTS_SUITES=tex_env"
run_case "CalDAV and TeX start the CalDAV stack and build the image" \
  "$CAL_DOWN,$CAL_UP,tex build,$CAL_RUN,$TEX_RUN,$CAL_DOWN" "SHALE_ENV_TESTS_SUITES=calendar::env_tests tex_env"
run_case "tex_install and tex_import ask for the image, and run on top of the list" \
  "$MAIL_DOWN,$MAIL_UP,tex build,$MAIL_RUN,cargo test --locked -- --ignored tex_install --test-threads=1,cargo test --locked -- --ignored tex_import --test-threads=1,$MAIL_DOWN" \
  "SHALE_ENV_TESTS_SUITES=mail_env" SHALE_ENV_TESTS_TEX_INSTALL=yes SHALE_ENV_TESTS_TEX_IMPORT=yes
run_case "the refresh flag reaches the image build" \
  "tex build --refresh,$TEX_RUN" "SHALE_ENV_TESTS_SUITES=tex_env" SHALE_ENV_TESTS_TEX_REFRESH=yes

# run_fails <name> <needle> [VAR=value…]: `run` fails at once, starting nothing
run_fails() {
  local name=$1 needle=$2 code
  shift 2
  N=$((N + 1))
  export SCENARIO="$TMP/r$N"
  mkdir -p "$SCENARIO"
  : > "$SCENARIO/calls"
  OUT=$(env -u SHALE_ENV_TESTS_SUITES "$@" PATH="$TMP/runbin:$PATH" SCENARIO="$SCENARIO" \
    SHALE_CALDAV_ENV_SH="$TMP/runbin/caldav-env.sh" SHALE_MAIL_ENV_SH="$TMP/runbin/mail-env.sh" \
    SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
    sh "$SCRIPT" run podman 2>&1)
  code=$?
  if [ "$code" = 1 ] && [ ! -s "$SCENARIO/calls" ] && printf '%s' "$OUT" | grep -Fq -- "$needle"; then
    echo "ok - dev_011: run $name"
  else
    echo "not ok - dev_011: run $name (exit $code, calls '$(paste -sd, "$SCENARIO/calls")'): $OUT"
    FAILED=1
  fi
}
run_fails "an unknown suite fails at once" "unknown suite: 'bogus'" "SHALE_ENV_TESTS_SUITES=mail_env bogus"
run_fails "an empty list fails at once" "is empty" "SHALE_ENV_TESTS_SUITES="

# the stack status on a failed start covers the stacks that were started
cat > "$TMP/runbin/mail-env.sh" <<'ENVSH'
#!/bin/sh
echo "mail $*" >> "$SCENARIO/calls"
[ "$1" != up ]
ENVSH
N=$((N + 1))
export SCENARIO="$TMP/r$N"
mkdir -p "$SCENARIO"
: > "$SCENARIO/calls"
OUT=$(env -u SHALE_ENV_TESTS_SUITES SHALE_ENV_TESTS_SUITES=mail_env PATH="$TMP/runbin:$PATH" SCENARIO="$SCENARIO" \
  SHALE_CALDAV_ENV_SH="$TMP/runbin/caldav-env.sh" SHALE_MAIL_ENV_SH="$TMP/runbin/mail-env.sh" \
  SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
  sh "$SCRIPT" run podman 2>&1)
code=$?
if [ "$code" = 1 ] && [ "$(paste -sd, "$SCENARIO/calls")" = "mail reset,mail up,mail status,mail reset" ]; then
  echo "ok - dev_011: run a failed stack start shows the status of the started stacks only"
else
  echo "not ok - dev_011: run failed start (exit $code): $(paste -sd, "$SCENARIO/calls") $OUT"
  FAILED=1
fi

exit "$FAILED"

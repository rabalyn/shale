#!/bin/bash
# Tests the retry of `.forgejo/scripts/env-tests.sh tex-suite` (spec DEV-011, DEV-035, #327): a fake `cargo`
# prints what each run of the tex_env suite says, a fake `tex-env.sh` records what is asked of it.
#   bash scripts/test-env-tests.sh
set -u
SCRIPT="$(cd "$(dirname "$0")/.." && pwd)/.forgejo/scripts/env-tests.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
# the tests use a lock file of their own, never the machine's (DEV-034)
export SHALE_ENV_LOCK_FILE="$TMP/env-stacks.lock"
unset SHALE_ENV_LOCK_TOKEN

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
# it sources the stack lock of #420 from tests/: copy that too
mkdir -p "$REPO/tests"
cp "$(dirname "$SCRIPT")/../../tests/env-lock.sh" "$REPO/tests/env-lock.sh"
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
ALL="calendar::env_tests mail_env tex_env forgejo_env"
suites_case "CalDAV code" "calendar::env_tests" src/calendar/ui.rs
suites_case "CalDAV environment" "calendar::env_tests" tests/caldav-env/caldav-env.sh
suites_case "mail code (src/mail.rs)" "mail_env" src/mail.rs
suites_case "mail code (src/mail/)" "mail_env" src/mail/imap.rs
suites_case "mail environment" "mail_env" tests/mail-env/mail-env.sh
suites_case "TeX code" "tex_env" src/pdf/tex/build.rs
suites_case "TeX assets" "tex_env" assets/tex/sets/x.txt
suites_case "TeX environment" "tex_env" tests/tex-env/tex-env.sh
suites_case "the sync wizard" "forgejo_env" src/gitsync/wizard/ui.rs
suites_case "the git credentials" "forgejo_env" src/gitsync/credentials.rs
suites_case "the Forgejo environment" "forgejo_env" tests/forgejo-env/forgejo-env.sh
suites_case "the askpass fake keyring" "forgejo_env" tests/askpass/secret-tool
suites_case "other git sync code needs none" "" src/gitsync/detect.rs
suites_case "CalDAV and TeX together, in a fixed order" "calendar::env_tests tex_env" src/pdf/tex/a.rs src/calendar/b.rs
suites_case "CalDAV, mail, TeX and Forgejo together" "$ALL" src/calendar/b.rs src/mail/c.rs tests/tex-env/d src/gitsync/wizard/e.rs
suites_case "the CI workflow runs all four" "$ALL" .forgejo/workflows/ci.yml
suites_case "env-tests.sh itself runs all four" "$ALL" .forgejo/scripts/env-tests.sh
suites_case "another file needs none" "" src/editor/x.rs docs/a.md
suites_case "another CI file needs none" "" .forgejo/scripts/check-coverage.sh

# --- `run` starts only the stacks its suites need (#447) ---
mkdir -p "$TMP/runbin"
cat > "$TMP/runbin/cargo" <<'CARGO'
#!/bin/sh
echo "cargo $*" >> "$SCENARIO/calls"
echo 'test result: ok. 3 passed; 0 failed; 0 ignored'
CARGO
for name in caldav mail tex forgejo; do
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
    SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_FORGEJO_ENV_SH="$TMP/runbin/forgejo-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
    sh "$SCRIPT" run podman 2>&1)
  code=$?
  # the helpers and the suites run at the same time (#448): the calls are compared as a set, the order is tested below
  got=$(sort "$SCENARIO/calls" | paste -sd,)
  want=$(printf '%s\n' "$want" | tr ',' '\n' | sort | paste -sd,)
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
FJ_UP="forgejo up"
FJ_RUN="cargo test --locked -- --ignored forgejo_env --test-threads=1"
FJ_DOWN="forgejo reset"
CAL_DOWN="caldav reset"
MAIL_DOWN="mail reset"
WARM="cargo test --locked --no-run"
run_case "unset runs all four suites on the three stacks and builds the image" \
  "$WARM,caldav reset,mail reset,$FJ_DOWN,$CAL_UP,$MAIL_UP,$FJ_UP,tex build,$CAL_RUN,$MAIL_RUN,$FJ_RUN,$TEX_RUN,$CAL_DOWN,$MAIL_DOWN,$FJ_DOWN"
run_case "a Forgejo list starts only the Forgejo stack, no image" \
  "$WARM,$FJ_DOWN,$FJ_UP,$FJ_RUN,$FJ_DOWN" "SHALE_ENV_TESTS_SUITES=forgejo_env"
run_case "a CalDAV list starts only the CalDAV stack, no image" \
  "$WARM,$CAL_DOWN,$CAL_UP,$CAL_RUN,$CAL_DOWN" "SHALE_ENV_TESTS_SUITES=calendar::env_tests"
run_case "a mail list starts only the mail stack, no image" \
  "$WARM,$MAIL_DOWN,$MAIL_UP,$MAIL_RUN,$MAIL_DOWN" "SHALE_ENV_TESTS_SUITES=mail_env"
run_case "a TeX list starts no stack and builds the image" \
  "$WARM,tex build,$TEX_RUN" "SHALE_ENV_TESTS_SUITES=tex_env"
run_case "CalDAV and TeX start the CalDAV stack and build the image" \
  "$WARM,$CAL_DOWN,$CAL_UP,tex build,$CAL_RUN,$TEX_RUN,$CAL_DOWN" "SHALE_ENV_TESTS_SUITES=calendar::env_tests tex_env"
run_case "tex_install and tex_import ask for the image, and run on top of the list" \
  "$WARM,$MAIL_DOWN,$MAIL_UP,tex build,$MAIL_RUN,cargo test --locked -- --ignored tex_install --test-threads=1,cargo test --locked -- --ignored tex_import --test-threads=1,$MAIL_DOWN" \
  "SHALE_ENV_TESTS_SUITES=mail_env" SHALE_ENV_TESTS_TEX_INSTALL=yes SHALE_ENV_TESTS_TEX_IMPORT=yes
run_case "the refresh flag reaches the image build" \
  "$WARM,tex build --refresh,$TEX_RUN" "SHALE_ENV_TESTS_SUITES=tex_env" SHALE_ENV_TESTS_TEX_REFRESH=yes

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
    SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_FORGEJO_ENV_SH="$TMP/runbin/forgejo-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
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
  SHALE_TEX_ENV_SH="$TMP/runbin/tex-env.sh" SHALE_FORGEJO_ENV_SH="$TMP/runbin/forgejo-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
  sh "$SCRIPT" run podman 2>&1)
code=$?
if [ "$code" = 1 ] && [ "$(paste -sd, "$SCENARIO/calls")" = "mail reset,mail up,mail status,mail reset" ]; then
  echo "ok - dev_011: run a failed stack start shows the status of the started stacks only"
else
  echo "not ok - dev_011: run failed start (exit $code): $(paste -sd, "$SCENARIO/calls") $OUT"
  FAILED=1
fi


# --- the parts of `run` go at the same time (DEV-011, #448) ---
mkdir -p "$TMP/pbin"
cat > "$TMP/pbin/cargo" <<'CARGO'
#!/bin/sh
# cargo test --locked -- --ignored <suite> --test-threads=<n>: three lines of output, FAKE_SLEEP apart; FAKE_FAIL names a suite that fails
if [ "$3" = --no-run ]; then
  echo "warmup begin" >> "$SCENARIO/events"
  sleep "${FAKE_SLEEP:-0}"
  echo "warmup end" >> "$SCENARIO/events"
  echo "warmup log line"
  if [ -n "${FAKE_FAIL_WARMUP:-}" ]; then echo "warmup broke" >&2; exit 101; fi
  exit 0
fi
suite=$5
echo "start $suite $6" >> "$SCENARIO/events"
for i in 1 2 3; do echo "out $suite $i"; sleep "${FAKE_SLEEP:-0}"; done
echo "end $suite" >> "$SCENARIO/events"
if [ "$suite" = "${FAKE_FAIL:-}" ]; then echo 'test result: FAILED. 0 passed; 1 failed; 0 ignored'; exit 101; fi
if [ "$suite" = "${FAKE_NONE:-}" ]; then echo 'test result: ok. 0 passed; 0 failed; 0 ignored'; exit 0; fi
echo 'test result: ok. 3 passed; 0 failed; 0 ignored'
CARGO
for name in caldav mail tex forgejo; do
  cat > "$TMP/pbin/$name-env.sh" <<'ENVSH'
#!/bin/sh
n=$(basename "$0" -env.sh)
echo "begin $n $1" >> "$SCENARIO/events"
case "$1" in reset|status) ;; *) sleep "${FAKE_SLEEP:-0}" ;; esac
echo "finish $n $1" >> "$SCENARIO/events"
case " ${FAKE_FAIL_HELPER:-} " in *" $n $1 "*) echo "$n $1 broke" >&2; exit 1 ;; esac
ENVSH
done
chmod +x "$TMP/pbin/"*

# par_run <name> [VAR=value…]: `run` with the fakes above; sets OUT, CODE, MS (milliseconds) and EVENTS (the file)
par_run() {
  shift # the name is for the reader of the test
  N=$((N + 1))
  export SCENARIO="$TMP/p$N"
  mkdir -p "$SCENARIO"
  : > "$SCENARIO/events"
  EVENTS="$SCENARIO/events"
  local t0 t1
  t0=$(date +%s%N)
  OUT=$(env -u SHALE_ENV_TESTS_SUITES "$@" PATH="$TMP/pbin:$PATH" SCENARIO="$SCENARIO" \
    SHALE_CALDAV_ENV_SH="$TMP/pbin/caldav-env.sh" SHALE_MAIL_ENV_SH="$TMP/pbin/mail-env.sh" \
    SHALE_TEX_ENV_SH="$TMP/pbin/tex-env.sh" SHALE_FORGEJO_ENV_SH="$TMP/pbin/forgejo-env.sh" SHALE_TEX_FAILED_LOGS="$SCENARIO/failed" \
    sh "$SCRIPT" run podman 2>&1)
  CODE=$?
  t1=$(date +%s%N)
  MS=$(((t1 - t0) / 1000000))
}
# check <name> <command>: ok when the command succeeds
check() {
  if eval "$2"; then echo "ok - dev_011: $1"; else echo "not ok - dev_011: $1 (exit $CODE, $MS ms): $OUT"; FAILED=1; fi
}
# the position of the first line of the events that equals $1 (0: none)
pos() { grep -nxF -- "$1" "$EVENTS" | head -n 1 | cut -d: -f1 | sed 's/^$/0/'; }
nopos() { local p; p=$(pos "$1"); echo "${p:-0}"; }

# spec: DEV-011 (#448): the stacks and the image start together, the suites run together: four suites of 1.5 s each, helpers of 0.5 s
par_run "all" FAKE_SLEEP=0.5
check "run: all four suites ran, with their own thread counts, and the run ends well" '[ "$CODE" = 0 ] && grep -qx "start calendar::env_tests --test-threads=1" "$EVENTS" && grep -qx "start mail_env --test-threads=1" "$EVENTS" && grep -qx "start forgejo_env --test-threads=1" "$EVENTS" && grep -qx "start tex_env --test-threads=4" "$EVENTS"'
# sequential: 3 x 0.5 s of helpers + 3 x 1.5 s of suites = 6 s; at the same time: 0.5 + 1.5 s plus the polling
check "run: the stacks, the image and the suites go at the same time (about as long as the slowest, not the sum)" '[ "$MS" -lt 3800 ]'
check "run: every helper started before the first suite and ended before it (the suites wait for the stacks)" '
  [ "$(nopos "begin caldav up")" -gt 0 ] && [ "$(nopos "begin mail up")" -gt 0 ] && [ "$(nopos "begin tex build")" -gt 0 ] &&
  [ "$(nopos "begin tex build")" -lt "$(nopos "finish caldav up")" ] && [ "$(nopos "begin mail up")" -lt "$(nopos "finish caldav up")" ] &&
  [ "$(nopos "finish caldav up")" -lt "$(nopos "start calendar::env_tests --test-threads=1")" ] &&
  [ "$(nopos "finish mail up")" -lt "$(nopos "start mail_env --test-threads=1")" ] &&
  [ "$(nopos "finish tex build")" -lt "$(nopos "start tex_env --test-threads=4")" ]'
check "run: the suites overlap (all started before the first ended)" '
  [ "$(nopos "start tex_env --test-threads=4")" -lt "$(nopos "end calendar::env_tests")" ] && [ "$(nopos "start mail_env --test-threads=1")" -lt "$(nopos "end calendar::env_tests")" ]'
# each log is printed on its own, in one piece
check "run: the output of a suite is not interleaved with another one" '
  [ "$(printf "%s\n" "$OUT" | grep "^out " | awk "{print \$2}" | uniq | wc -l)" = 4 ] && [ "$(printf "%s\n" "$OUT" | grep -c "^out ")" = 12 ]'
check "run: every log has a heading with its exit code" '
  printf "%s\n" "$OUT" | grep -q "^===== calendar::env_tests: exit 0" && printf "%s\n" "$OUT" | grep -q "^===== mail_env: exit 0" && printf "%s\n" "$OUT" | grep -q "^===== tex_env: exit 0" && printf "%s\n" "$OUT" | grep -q "^===== forgejo_env: exit 0"'

# one suite fails: the others still end and are reported, and the failing one is named
par_run "one failing" FAKE_SLEEP=0.2 FAKE_FAIL=mail_env
check "run: a failing suite fails the run and is named" '[ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "^FAILED suites: mail_env$"'
check "run: the other suites still ran to the end and are reported with their results" '
  grep -qx "end calendar::env_tests" "$EVENTS" && grep -qx "end tex_env" "$EVENTS" &&
  printf "%s\n" "$OUT" | grep -q "^===== calendar::env_tests: exit 0" && printf "%s\n" "$OUT" | grep -q "^===== tex_env: exit 0" &&
  printf "%s\n" "$OUT" | grep -q "^===== mail_env: exit 1" && printf "%s\n" "$OUT" | grep -q "test result: FAILED"'
par_run "two failing" FAKE_SLEEP=0 FAKE_FAIL=calendar::env_tests SHALE_ENV_TESTS_SUITES="calendar::env_tests mail_env"
check "run: a suite that fails and one that passes: only the failing one is named" '[ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "^FAILED suites: calendar::env_tests$"'
# a suite that ran no test (a filter that matches nothing) is a failure of that suite
par_run "no test" FAKE_NONE=mail_env SHALE_ENV_TESTS_SUITES="calendar::env_tests mail_env"
check "run: a suite that ran no test fails on its own, as before" '[ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "^no mail_env test passed$" && printf "%s\n" "$OUT" | grep -q "^FAILED suites: mail_env$"'

# a stack that does not come up: the other parts of the start are waited for, the status of the stacks is shown, no suite starts
par_run "stack failing" FAKE_FAIL_HELPER="caldav up"
check "run: a stack that does not come up is named, the status of both stacks is shown and no suite starts" '
  [ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "a stack did not come up: up-caldav" && printf "%s\n" "$OUT" | grep -q "caldav up broke" &&
  grep -qx "finish tex build" "$EVENTS" && grep -qx "begin caldav status" "$EVENTS" && grep -qx "begin mail status" "$EVENTS" && ! grep -q "^start " "$EVENTS"'
check "run: and the stacks are taken down again" '[ "$(grep -c "^begin caldav reset" "$EVENTS")" = 2 ] && [ "$(grep -c "^begin mail reset" "$EVENTS")" = 2 ]'
par_run "image failing" FAKE_FAIL_HELPER="tex build"
check "run: an image that cannot be built fails the run before any suite" '
  [ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "the TeX image could not be built" && ! grep -q "^start " "$EVENTS"'

# the warm-up (#462): one cargo before any suite, so a fresh toolchain is installed once
par_run "warm-up" FAKE_SLEEP=0.1
check "run: the warm-up cargo test --locked --no-run runs once, before any suite and after the stacks and the image" '
  [ "$CODE" = 0 ] && [ "$(grep -c "^warmup begin$" "$EVENTS")" = 1 ] && [ "$(nopos "warmup end")" -gt 0 ] &&
  [ "$(nopos "finish tex build")" -lt "$(nopos "warmup begin")" ] && [ "$(nopos "finish caldav up")" -lt "$(nopos "warmup begin")" ] &&
  [ "$(nopos "warmup end")" -lt "$(nopos "start calendar::env_tests --test-threads=1")" ] &&
  [ "$(nopos "warmup end")" -lt "$(nopos "start mail_env --test-threads=1")" ] &&
  [ "$(nopos "warmup end")" -lt "$(nopos "start tex_env --test-threads=4")" ]'
check "run: a successful warm-up does not print its log" '! printf "%s\n" "$OUT" | grep -q "warmup log line"'
par_run "warm-up failing" FAKE_FAIL_WARMUP=yes
check "run: a failing warm-up fails the run, names the warm-up, shows its log and starts no suite" '
  [ "$CODE" = 1 ] && printf "%s\n" "$OUT" | grep -q "the cargo warm-up (cargo test --locked --no-run) failed" &&
  printf "%s\n" "$OUT" | grep -q "warmup broke" && ! grep -q "^start " "$EVENTS"'
check "run: and the stacks are taken down after a failing warm-up" '[ "$(grep -c "^begin caldav reset" "$EVENTS")" = 2 ] && [ "$(grep -c "^begin mail reset" "$EVENTS")" = 2 ]'

# tex_install and tex_import share one install of TeX Live: one after the other, but beside the other suites
par_run "install and import" FAKE_SLEEP=0.3 SHALE_ENV_TESTS_SUITES="mail_env" SHALE_ENV_TESTS_TEX_INSTALL=yes SHALE_ENV_TESTS_TEX_IMPORT=yes
check "run: tex_install ends before tex_import starts" '
  [ "$CODE" = 0 ] && [ "$(nopos "end tex_install")" -gt 0 ] && [ "$(nopos "end tex_install")" -lt "$(nopos "start tex_import --test-threads=1")" ]'
check "run: and they run beside the other suites" '[ "$(nopos "start tex_install --test-threads=1")" -lt "$(nopos "end mail_env")" ] && printf "%s\n" "$OUT" | grep -q "^===== tex_install+tex_import: exit 0"'
par_run "install failing" FAKE_FAIL=tex_install SHALE_ENV_TESTS_TEX_INSTALL=yes SHALE_ENV_TESTS_TEX_IMPORT=yes SHALE_ENV_TESTS_SUITES="mail_env"
check "run: a failed tex_install still lets tex_import run, and names the pair" '
  [ "$CODE" = 1 ] && grep -qx "end tex_import" "$EVENTS" && printf "%s\n" "$OUT" | grep -q "^FAILED suites: tex_install+tex_import$"'



# spec: DEV-034: one checkout at a time uses the podman stacks (#420). `run` is started with fake helpers
# (the real ones are in a copy of the tree), a fake cargo and a fake container engine.
LT="$TMP/lt"
mkdir -p "$LT/.forgejo/scripts" "$LT/tests/mail-env" "$LT/tests/caldav-env" "$LT/tests/tex-env" "$LT/tests/forgejo-env" "$LT/lbin"
cp "$SCRIPT" "$LT/.forgejo/scripts/env-tests.sh"
cp "$(dirname "$SCRIPT")/../../tests/env-lock.sh" "$LT/tests/env-lock.sh"
for h in mail-env caldav-env tex-env forgejo-env; do
  # each helper records that it ran and whether it saw the held marker
  cat > "$LT/tests/$h/$h.sh" <<'HELPER'
#!/bin/sh
echo "$(basename "$0") $* token=${SHALE_ENV_LOCK_TOKEN:-none}" >> "$LOG"
HELPER
done
cat > "$LT/lbin/cargo" <<'CARGO'
#!/bin/sh
echo "cargo $*" >> "$LOG"
echo "test result: ok. 1 passed; 0 failed; 0 ignored"
CARGO
chmod +x "$LT/lbin/cargo"
# a helper that takes the lock the way the real ones do, to see a child under the holder
cat > "$LT/child.sh" <<'CHILD'
#!/bin/sh
. "$(dirname "$0")/tests/env-lock.sh"
shale_env_lock
echo "child got through" >> "$LOG"
CHILD

lt_run() { # runs `env-tests.sh run podman` in the copy, in the background; $1 is the log
  (cd "$LT" && PATH="$LT/lbin:$PATH" LOG="$1" SHALE_ENV_TESTS_TEX_THREADS=1 sh .forgejo/scripts/env-tests.sh run podman > "$1.out" 2>&1; echo $? > "$1.rc") &
}
lt_wait_for() { # <file> <text>: up to 10 s
  i=0; while [ "$i" -lt 100 ]; do grep -Fq -- "$2" "$1" 2>/dev/null && return 0; i=$((i + 1)); sleep 0.1; done; return 1
}
lt_check() { # <name> <command>
  N=$((N + 1))
  if eval "$2"; then echo "ok $N - $1"; else echo "not ok $N - $1"; FAILED=1; fi
}

if command -v flock >/dev/null 2>&1; then
  # a first holder (another checkout) has the lock
  : > "$TMP/hold.ready"
  (exec 8>>"$SHALE_ENV_LOCK_FILE"; flock 8; echo held > "$TMP/hold.ready"; sleep 30) &
  HOLDER=$!
  lt_wait_for "$TMP/hold.ready" held
  lt_run "$TMP/second.log"
  lt_wait_for "$TMP/second.log.out" "waiting for the podman test stacks, used by another checkout"
  lt_check "dev_034: a second env-tests.sh run says it waits for the stacks" '[ -f "$TMP/second.log.out" ] && grep -Fq "waiting for the podman test stacks, used by another checkout" "$TMP/second.log.out"'
  sleep 1
  lt_check "dev_034: and has started nothing while the lock is held elsewhere" '[ ! -s "$TMP/second.log" ] && [ ! -f "$TMP/second.log.rc" ]'
  # the other checkout lets go: the run goes on
  pkill -P "$HOLDER" sleep 2>/dev/null; kill "$HOLDER" 2>/dev/null; wait "$HOLDER" 2>/dev/null
  lt_wait_for "$TMP/second.log.rc" 0
  lt_check "dev_034: when the lock is free the run starts and ends well" '[ "$(cat "$TMP/second.log.rc" 2>/dev/null)" = 0 ] && grep -q "^cargo " "$TMP/second.log"'
  lt_check "dev_034: the helpers the run starts saw the held marker (they would deadlock on it otherwise)" '[ "$(grep -c "token=none" "$TMP/second.log")" = 0 ] && grep -q "mail-env.sh up token=." "$TMP/second.log"'
  wait

  # a child that takes the lock while its parent holds it does not wait for it
  rm -f "$TMP/nest.log"
  (. "$LT/tests/env-lock.sh"; shale_env_lock; LOG="$TMP/nest.log"; export LOG; timeout 10 sh "$LT/child.sh"; echo "rc=$?" >> "$TMP/nest.log")
  lt_check "dev_034: a child started by the holder does not wait for its parent" 'grep -q "child got through" "$TMP/nest.log" && grep -q "rc=0" "$TMP/nest.log"'

  # a stale marker (the holder is gone) and a foreign one (another holder wrote another token) skip nothing
  rm -f "$TMP/stale.log"
  printf 'stale-token\n' > "$SHALE_ENV_LOCK_FILE"
  (LOG="$TMP/stale.log" SHALE_ENV_LOCK_TOKEN=stale-token timeout 10 sh "$LT/child.sh" 2> "$TMP/stale.err")
  lt_check "dev_034: a stale marker (nobody holds the lock) is no reason to skip it: the lock is taken, with a new token" 'grep -q "child got through" "$TMP/stale.log" && [ "$(cat "$SHALE_ENV_LOCK_FILE")" != stale-token ]'
  rm -f "$TMP/foreign.log"
  (exec 8>>"$SHALE_ENV_LOCK_FILE"; flock 8; printf 'holder-token\n' > "$SHALE_ENV_LOCK_FILE"; sleep 30) &
  HOLDER=$!
  sleep 1
  (LOG="$TMP/foreign.log" SHALE_ENV_LOCK_TOKEN=inherited-token timeout 3 sh "$LT/child.sh" 2> "$TMP/foreign.err")
  lt_check "dev_034: an inherited marker that is not the holder's token still waits for the lock" '[ ! -s "$TMP/foreign.log" ] && grep -Fq "waiting for the podman test stacks" "$TMP/foreign.err"'
  pkill -P "$HOLDER" sleep 2>/dev/null; kill "$HOLDER" 2>/dev/null; wait "$HOLDER" 2>/dev/null
else
  echo "ok - dev_034: skipped, flock is not installed here"
fi

exit "$FAILED"

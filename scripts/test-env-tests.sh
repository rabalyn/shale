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



# spec: DEV-034: one checkout at a time uses the podman stacks (#420). `run` is started with fake helpers
# (the real ones are in a copy of the tree), a fake cargo and a fake container engine.
LT="$TMP/lt"
mkdir -p "$LT/.forgejo/scripts" "$LT/tests/mail-env" "$LT/tests/caldav-env" "$LT/tests/tex-env" "$LT/lbin"
cp "$SCRIPT" "$LT/.forgejo/scripts/env-tests.sh"
cp "$(dirname "$SCRIPT")/../../tests/env-lock.sh" "$LT/tests/env-lock.sh"
for h in mail-env caldav-env tex-env; do
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

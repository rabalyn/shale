#!/bin/bash
# Lands branches on `main` locally (FORGE-011, ADR-0016): for each branch, in the order given,
# it merges the branch into the current `origin/main` in a worktree of its own, runs the checks
# there and pushes `main` only when all of them pass. One landing at a time: a lock in the git
# folder makes a second `land.sh` (another agent) wait.
#
#   scripts/land.sh [--release] [--no-env-tests] <branch>…
#
# A branch is a local branch if one has the name, else `origin/<branch>`, else any commit.
# The checks: every new commit names an issue (FORGE-008), `cargo fmt --check`, clippy with
# `-D warnings` for Linux (also with `--features bench`), and for macOS and Windows (`scripts/cross-check.sh`), `cargo nextest run --locked --no-fail-fast` (#451; cargo-nextest is required, a landing without it stops at the start; `--test-threads` from RUST_TEST_THREADS or the cargo config's [env], because nextest does not read it itself; also `cargo test --features bench bench::` when the change touches the harness or the build, `scripts/bench-tests-wanted.sh` (BUILD-035, #449), or with --release, and also `cargo test --release` with --release), and the real-server
# suites of tests/caldav-env, tests/mail-env and tests/forgejo-env and the LuaLaTeX suite of tests/tex-env, each only when the
# change touches its code or environment (`.forgejo/scripts/env-tests.sh suites`, #447); --no-env-tests skips them and
# says so. A merge conflict is not resolved here: merge `origin/main` into the branch, resolve
# it there and run land.sh again. After the push the local branch `main` is fast-forwarded to
# the landed commit (in the worktree that has it checked out, when that one is clean), so new
# sessions start from it; when it has commits of its own or its checkout has changes, it is left
# as it is and the script says why (ADR-0018).
#
# The worktree and the build folder stay in `<git dir>/land/`, so the next landing builds only
# what changed. Environment, for the test of this script: LAND_REMOTE (default origin),
# LAND_CHECK_CMD (a shell command run in the worktree instead of the checks above),
# SHALE_LAND_LOCK_HELD=<pid of the parent> (the parent holds the lock already: release scripts).
#
# A TeX Live mirror (DEV-025, #371): the real TeX Live install test and the TeX test image take TeX
# Live from the public default unless a local setting names a mirror of `tlnet`: the environment
# variable SHALE_TEX_MIRROR (a URL), else the first line of `<git dir>/land/tex-mirror` that is not
# empty or a `#` comment. The file is in the git folder, so it is never committed and every worktree
# shares it. The URL starts with https:// or http:// (else exit 2); a trailing / is dropped. It is
# passed on as SHALE_TEX_INSTALL_REPOSITORY and SHALE_TEX_ENV_REPOSITORY (not when the person set
# those already). The line "TeX Live: …" at the start says which one is used. CI does not read it.
set -euo pipefail

REMOTE=${LAND_REMOTE:-origin}
RELEASE=no
ENV_TESTS=yes
BRANCHES=()
for a in "$@"; do
  case $a in
    --release) RELEASE=yes ;;
    --no-env-tests) ENV_TESTS=no ;;
    -h|--help) sed -n '2,31p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "land.sh: unknown option $a" >&2; exit 2 ;;
    *) BRANCHES+=("$a") ;;
  esac
done
if [ ${#BRANCHES[@]} -eq 0 ]; then
  echo "usage: scripts/land.sh [--release] [--no-env-tests] <branch>…" >&2
  exit 2
fi

# The main test step runs with cargo-nextest (FORGE-011, #451): no silent fallback to `cargo test`,
# so a missing tool stops the landing before anything is merged or built. (LAND_CHECK_CMD replaces
# the checks, so it needs no nextest.)
if [ -z "${LAND_CHECK_CMD:-}" ] && ! cargo nextest --version >/dev/null 2>&1; then
  echo "land.sh: cargo-nextest is missing: install it with: cargo install cargo-nextest --locked" >&2
  exit 2
fi

COMMON=$(git rev-parse --path-format=absolute --git-common-dir)
HOME_DIR="$COMMON/land"
WT="$HOME_DIR/worktree"
mkdir -p "$HOME_DIR"
export CARGO_TARGET_DIR="${LAND_TARGET_DIR:-$HOME_DIR/target}"

# The TeX Live mirror (DEV-025, #371): SHALE_TEX_MIRROR, else the file `tex-mirror` in $HOME_DIR.
# Nothing is exported when neither is set; a SHALE_TEX_INSTALL_REPOSITORY or
# SHALE_TEX_ENV_REPOSITORY the person set is left alone.
TEX_MIRROR_FILE="$HOME_DIR/tex-mirror"
tex_mirror=""
tex_mirror_from=""
if [ -n "${SHALE_TEX_MIRROR:-}" ]; then
  tex_mirror=$SHALE_TEX_MIRROR
  tex_mirror_from="SHALE_TEX_MIRROR"
elif [ -f "$TEX_MIRROR_FILE" ]; then
  tex_mirror=$(sed -e 's/\r$//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' "$TEX_MIRROR_FILE" | awk 'NF && !/^#/ { print; exit }')
  [ -z "$tex_mirror" ] || tex_mirror_from=$TEX_MIRROR_FILE
fi
if [ -n "$tex_mirror" ]; then
  case $tex_mirror in
    https://?*|http://?*) ;;
    *) echo "land.sh: the TeX Live mirror (from $tex_mirror_from) must start with https:// or http://" >&2; exit 2 ;;
  esac
  tex_mirror=${tex_mirror%/}
  echo "TeX Live: the mirror $tex_mirror (from $tex_mirror_from)"
  for v in SHALE_TEX_INSTALL_REPOSITORY SHALE_TEX_ENV_REPOSITORY; do
    if [ -n "${!v:-}" ]; then
      echo "TeX Live: $v is set already, left as it is"
    else
      export "$v=$tex_mirror"
    fi
  done
else
  echo "TeX Live: the default repository (no mirror set: SHALE_TEX_MIRROR or $TEX_MIRROR_FILE)"
fi

# scripts/make-tag.sh and make-release.sh hold this lock for their whole run (FORGE-013, FORGE-014,
# #348) and start land.sh as their own child with SHALE_LAND_LOCK_HELD set to their PID: that is
# trusted only when it is this script's parent, so a stale or inherited value never skips the lock.
if [ "${SHALE_LAND_LOCK_HELD:-}" = "$PPID" ]; then
  echo "(the landing lock is held by the release script that started this)"
else
  exec 9>"$HOME_DIR/lock"
  if ! flock -n 9; then
    echo "Another landing is running; waiting for it…"
    flock 9
  fi
fi

remove_worktree() {
  if [ -e "$WT" ]; then git worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; fi
  git worktree prune
}

# Statistics of the landing (FORGE-015, #368): `current` says what runs now (start, branch, step,
# step start; tab-separated, written atomically), `current.steps` collects `name<TAB>seconds<TAB>
# status` of every step (the checks run in a subshell, so it is a file), and when a branch's
# landing ends one line goes to `history.tsv`. None of this may fail a landing: errors are ignored.
CURRENT="$HOME_DIR/current"
STEPS="$HOME_DIR/current.steps"
HISTORY="$HOME_DIR/history.tsv"
CUR_BRANCH=""
CUR_START=0
RESULT=error
STEP_NAME=""
STEP_T0=0

write_current() {
  { printf '%s\t%s\t%s\t%s\n' "$CUR_START" "$CUR_BRANCH" "${1//$'\t'/ }" "$2" > "$CURRENT.tmp" && mv -f "$CURRENT.tmp" "$CURRENT"; } 2>/dev/null || true
}
record_step() {
  { printf '%s\t%s\t%s\n' "${1//$'\t'/ }" "$(($(date +%s) - $2))" "$3" >> "$STEPS"; } 2>/dev/null || true
}
begin_step() {
  STEP_NAME=$1
  STEP_T0=$(date +%s)
  write_current "$STEP_NAME" "$STEP_T0"
}
end_step() {
  [ -n "$STEP_NAME" ] || return 0
  record_step "$STEP_NAME" "$STEP_T0" "$1"
  STEP_NAME=""
}
start_landing() {
  CUR_BRANCH=$1
  CUR_START=$(date +%s)
  RESULT=error
  STEP_NAME=""
  { : > "$STEPS"; } 2>/dev/null || true
}
finish_landing() {
  [ -n "$CUR_BRANCH" ] || return 0
  end_step 1
  {
    local end load failed fields
    end=$(date +%s)
    load=$(awk '{print $1}' /proc/loadavg 2>/dev/null || true)
    failed=""
    [ "$RESULT" = failed ] && failed=$(awk -F'\t' '$3 != 0 { print $1; exit }' "$STEPS" 2>/dev/null)
    fields=$(awk -F'\t' '{ printf "\t%s=%s", $1, $2 }' "$STEPS" 2>/dev/null)
    printf '%s\t%s\t%s\t%s\t%s\t%s%s\n' "$CUR_START" "$end" "$CUR_BRANCH" "$RESULT" "$failed" "$load" "$fields" >> "$HISTORY"
  } 2>/dev/null || true
  CUR_BRANCH=""
  rm -f "$CURRENT" "$CURRENT.tmp" "$STEPS" 2>/dev/null || true
}
on_exit() {
  finish_landing
  remove_worktree
}
trap on_exit EXIT

# One check: its name, then the command. A failing one ends the checks with its name. Every
# step returns its own status, so this holds without `set -e`, which bash ignores in a function
# called from `if` (a failing `cargo test` once let a branch land).
step() {
  local name=$1 t0 rc=0
  shift
  echo "-- $name"
  t0=$(date +%s)
  write_current "$name" "$t0"
  "$@" || rc=$?
  record_step "$name" "$t0" "$rc"
  if [ "$rc" != 0 ]; then
    echo "land.sh: check failed: $name" >&2
    return 1
  fi
}

# The checks of FORGE-011 on the merged tree in the current folder. LAND_CHECK_CMD replaces them
# with its lines, each one a step run by `sh -c` (the test of this script).
checks() {
  local base=$1 line
  if [ -n "${LAND_CHECK_CMD:-}" ]; then
    while IFS= read -r line; do
      step "$line" sh -c "$line" || return 1
    done <<< "$LAND_CHECK_CMD"
    return 0
  fi
  step "commits name an issue (FORGE-008)" sh .forgejo/scripts/check-commit-issues.sh "$base" HEAD || return 1
  step "cargo fmt --check" cargo fmt --check || return 1
  step "cargo clippy" cargo clippy -q --all-targets --locked -- -D warnings || return 1
  # the benchmark harness is a cargo feature that no shipped build has: it must not rot (BUILD-035)
  step "cargo clippy --features bench" cargo clippy -q --all-targets --locked --features bench -- -D warnings || return 1
  # code used only from Linux-only `cfg` code is dead on macOS and Windows (BUILD-004, DEV-015)
  step "macOS and Windows clippy (scripts/cross-check.sh)" sh scripts/cross-check.sh || return 1
  # nextest runs all tests in one pool, ~30 % faster than libtest (#451); it does not read
  # RUST_TEST_THREADS (only NEXTEST_TEST_THREADS), so the thread budget is passed on: the variable,
  # else the `RUST_TEST_THREADS = "N"` of the cargo config's [env] (which only `cargo test` applies
  # to libtest, so this shell does not have it), else nextest's default (all cores)
  local nextest_threads=() threads=${RUST_TEST_THREADS:-}
  if [ -z "$threads" ]; then
    threads=$(command grep -hs -m1 '^[[:space:]]*RUST_TEST_THREADS[[:space:]]*=' "${CARGO_HOME:-$HOME/.cargo}/config.toml" .cargo/config.toml | sed 's/[^0-9]//g' | head -n 1 || true)
  fi
  [ -z "$threads" ] || nextest_threads=(--test-threads "$threads")
  step "cargo nextest run" cargo nextest run --locked --no-fail-fast --status-level fail "${nextest_threads[@]}" || return 1
  # its tests only when the change touches the harness or the build, and with --release (BUILD-035, #449)
  if [ "$RELEASE" = yes ] || [ "$(sh scripts/bench-tests-wanted.sh "$base" HEAD)" = yes ]; then
    step "cargo test --features bench bench::" cargo test -q --locked --features bench bench:: || return 1
  else
    echo "-- SKIPPED (the change touches neither the benchmark harness nor the build): cargo test --features bench bench:: did not run"
  fi
  if [ "$RELEASE" = yes ]; then
    step "cargo test --release" cargo test -q --release --locked || return 1
  fi
  # only the suites the change touches (#447): CalDAV, mail, TeX export, or all of them for the CI files
  env_suites=$(sh .forgejo/scripts/env-tests.sh suites "$base" HEAD)
  if [ -n "$env_suites" ]; then
    if [ "$ENV_TESTS" = no ]; then
      echo "-- SKIPPED (--no-env-tests): the change touches CalDAV, mail or TeX export, the real-server suites did not run"
    else
      # the real install of TeX Live is heavy: only when the change touches it
      install_tests=$(sh .forgejo/scripts/env-tests.sh install-wanted "$base" HEAD)
      # and so are the downloads of the template sets
      import_tests=$(sh .forgejo/scripts/env-tests.sh import-wanted "$base" HEAD)
      step "real-server suites (podman)" env SHALE_ENV_TESTS_SUITES="$env_suites" SHALE_ENV_TESTS_TEX_INSTALL="$install_tests" SHALE_ENV_TESTS_TEX_IMPORT="$import_tests" sh .forgejo/scripts/env-tests.sh run || return 1
    fi
  fi
}

# The commit to land: of a local branch and `origin/<branch>` the newer one (the other is behind
# it); when both have commits of their own there is no answer, and the landing stops.
resolve() {
  local b=$1 local_ref="" remote_ref=""
  local_ref=$(git rev-parse --verify -q "refs/heads/$b^{commit}" || true)
  remote_ref=$(git rev-parse --verify -q "refs/remotes/$REMOTE/$b^{commit}" || true)
  if [ -n "$local_ref" ] && [ -n "$remote_ref" ]; then
    if git merge-base --is-ancestor "$remote_ref" "$local_ref"; then
      echo "$local_ref"
    elif git merge-base --is-ancestor "$local_ref" "$remote_ref"; then
      echo "$remote_ref"
    else
      echo "land.sh: $b and $REMOTE/$b have both moved; bring them together first" >&2
      return 1
    fi
  elif [ -n "$local_ref$remote_ref" ]; then
    echo "$local_ref$remote_ref"
  elif git rev-parse --verify -q "$b^{commit}" >/dev/null; then
    git rev-parse "$b^{commit}"
  else
    return 1
  fi
}

# Moves the local branch `main` up to the landed commit $1 (FORGE-011, ADR-0018). Never fails
# the landing: what it cannot do, it says.
update_local_main() {
  local new=$1 cur wt="" path="" line
  cur=$(git rev-parse -q --verify refs/heads/main) || return 0
  [ "$cur" = "$new" ] && return 0
  if ! git merge-base --is-ancestor "$cur" "$new"; then
    echo "note: local main has commits that are not on $REMOTE/main; it was left at $(git rev-parse --short "$cur")"
    return 0
  fi
  while IFS= read -r line; do
    case $line in
      "worktree "*) path=${line#worktree } ;;
      "branch refs/heads/main") wt=$path ;;
    esac
  done < <(git worktree list --porcelain)
  if [ -z "$wt" ]; then
    git update-ref refs/heads/main "$new" "$cur"
  elif [ -n "$(git -C "$wt" status --porcelain --untracked-files=no)" ]; then
    echo "note: local main was left at $(git rev-parse --short "$cur"): its checkout $wt has uncommitted changes"
    return 0
  elif ! git -C "$wt" merge -q --ff-only "$new" >/dev/null 2>&1; then
    echo "note: local main was left at $(git rev-parse --short "$cur"): the fast-forward in $wt failed (untracked files in the way?)"
    return 0
  fi
  echo "local main is now $(git rev-parse --short "$new")"
}

for b in "${BRANCHES[@]}"; do
  echo "== $b"
  start_landing "$b"
  begin_step merge
  git fetch -q "$REMOTE"
  if ! ref=$(resolve "$b"); then
    echo "land.sh: no branch or commit named $b" >&2
    exit 1
  fi
  base=$(git rev-parse "refs/remotes/$REMOTE/main")
  if git merge-base --is-ancestor "$ref" "$base"; then
    echo "already on $REMOTE/main, nothing to land"
    STEP_NAME=""
    RESULT=nothing
    finish_landing
    continue
  fi
  remove_worktree
  git worktree add -q --detach "$WT" "$base"
  if ! git -C "$WT" merge -q --no-ff --no-edit -m "Merge branch '$b' (landed locally, FORGE-011)" "$ref" >/dev/null 2>&1; then
    echo "land.sh: $b conflicts with $REMOTE/main in:" >&2
    git -C "$WT" diff --name-only --diff-filter=U | sed 's/^/  /' >&2
    echo "Merge $REMOTE/main into $b, resolve the conflict there and run land.sh again. $REMOTE/main is unchanged." >&2
    RESULT=conflict
    exit 1
  fi
  end_step 0
  if ! (cd "$WT" && checks "$base"); then
    echo "land.sh: a check failed for $b; $REMOTE/main is unchanged." >&2
    RESULT=failed
    exit 1
  fi
  begin_step push
  if ! git -C "$WT" push -q "$REMOTE" HEAD:main; then
    echo "land.sh: $REMOTE/main moved while $b was checked (pushed by hand?); run land.sh again." >&2
    RESULT=failed
    exit 1
  fi
  end_step 0
  echo "landed $b as $(git -C "$WT" rev-parse --short HEAD) on $REMOTE/main"
  RESULT=landed
  update_local_main "$(git -C "$WT" rev-parse HEAD)"
  finish_landing
done

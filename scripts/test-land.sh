#!/bin/bash
# Tests scripts/land.sh on a throwaway remote and clone (spec FORGE-011). The cargo checks are
# replaced by LAND_CHECK_CMD, so the test is about merging, refusing and pushing.
#   bash scripts/test-land.sh
set -u
LAND="$(cd "$(dirname "$0")" && pwd)/land.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
export GIT_CONFIG_GLOBAL="$TMP/gitconfig"
git config --global user.email t@example.com
git config --global user.name test
git config --global commit.gpgsign false
git config --global init.defaultBranch main

git init -q --bare "$TMP/remote.git"
git clone -q "$TMP/remote.git" "$TMP/work" 2>/dev/null
cd "$TMP/work" || exit 1
printf 'one\n' > a.txt
git add a.txt && git commit -q -m "Start (Refs #1)" && git push -q origin main

# branch <name> <file> <text>: a branch from main with one commit, pushed
branch() {
  git checkout -q -b "$1" origin/main
  printf '%s\n' "$3" > "$2"
  git add "$2" && git commit -q -m "$1 (Refs #1)"
  git push -q origin "$1"
  git checkout -q --detach
}

# expect <name> <0|1> <needle> <args>…: runs land.sh, checks the exit code and the output
expect() {
  local name=$1 code=$2 needle=$3
  shift 3
  N=$((N + 1))
  OUT=$(bash "$LAND" "$@" 2>&1)
  GOT=$?
  if [ "$GOT" = "$code" ] && printf '%s' "$OUT" | grep -Fq -- "$needle"; then
    echo "ok $N - $name"
  else
    echo "not ok $N - $name (exit $GOT): $OUT"
    FAILED=1
  fi
}
main_is() { git fetch -q origin; [ "$(git rev-parse origin/main)" = "$1" ]; }
check() {
  N=$((N + 1))
  if eval "$2"; then echo "ok $N - $1"; else echo "not ok $N - $1"; FAILED=1; fi
}

export LAND_CHECK_CMD=true
branch feature-b b.txt b
branch feature-c c.txt c
expect "two branches land in the order given" 0 "landed feature-c" feature-b feature-c
check "main holds both, as merge commits" '[ "$(git show origin/main:b.txt)" = b ] && [ "$(git show origin/main:c.txt)" = c ] && git log -1 --format=%s origin/main | grep -Fq "Merge branch '"'"'feature-c'"'"' (landed locally, FORGE-011)"'
expect "a landed branch is not landed twice" 0 "already on origin/main" feature-b

branch clash-1 a.txt left
git checkout -q -b clash-2 origin/main && printf 'right\n' > a.txt && git commit -qam "clash-2 (Refs #1)" && git checkout -q --detach
expect "the first of two clashing branches lands" 0 "landed clash-1" clash-1
before=$(git rev-parse origin/main)
expect "a conflict is refused and named" 1 "a.txt" clash-2
check "a conflict leaves main as it was" "main_is $before"

branch broken d.txt d
export LAND_CHECK_CMD=false
expect "a failing check pushes nothing" 1 "a check failed for broken" broken
check "main is unchanged after a failing check" "main_is $before"
# a failing step stops the checks even when a later one passes (the bug that landed a red tree)
export LAND_CHECK_CMD=$'false\ntrue'
expect "a failing step is not hidden by a passing one after it" 1 "check failed: false" broken
check "main is unchanged after a failing step" "main_is $before"
export LAND_CHECK_CMD='test -f d.txt'
expect "the checks run in the merged tree" 0 "landed broken" broken

export LAND_CHECK_CMD=true
branch moved e.txt e
git checkout -q moved && printf 'e2\n' > e.txt && git commit -qam "moved again (Refs #1)" && git checkout -q --detach
git fetch -q origin
git checkout -q -b diverged origin/main && printf 'x\n' > x.txt && git add x.txt && git commit -qm "x (Refs #1)" && git push -q origin diverged
git reset -q --hard origin/main && printf 'y\n' > y.txt && git add y.txt && git commit -qm "y (Refs #1)" && git checkout -q --detach
expect "of a local branch and its remote copy the newer one lands" 0 "landed moved" moved
check "the local commit that was not pushed is on main" '[ "$(git show origin/main:e.txt)" = e2 ]'
expect "a branch that differs locally and remotely is refused" 1 "have both moved" diverged
expect "an unknown name is refused" 1 "no branch or commit named nope" nope
check "no landing worktree is left behind" '! git worktree list | grep -Fq land/worktree'

exit "$FAILED"

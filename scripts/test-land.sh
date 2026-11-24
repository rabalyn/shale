#!/bin/bash
# Tests scripts/land.sh on a throwaway remote and clone (spec FORGE-011). The cargo checks are
# replaced by LAND_CHECK_CMD, so the test is about merging, refusing and pushing.
#   bash scripts/test-land.sh
set -u
# the person's own settings must not leak into the test (DEV-025)
unset SHALE_TEX_MIRROR SHALE_TEX_INSTALL_REPOSITORY SHALE_TEX_ENV_REPOSITORY
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
# FORGE-015: the history of landings and the file of the running one
LANDDIR="$TMP/work/.git/land"
last_hist() { tail -n 1 "$LANDDIR/history.tsv"; }
# hist_is <result> <branch> <failed step>: the last line of history.tsv says so (start and end are
# numbers, the load a number or empty) and `current` is gone
hist_is() {
  last_hist | awk -F'\t' -v r="$1" -v b="$2" -v f="$3" '$1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ && $2 >= $1 && $3 == b && $4 == r && $5 == f && ($6 == "" || $6 ~ /^[0-9.]+$/) { ok = 1 } END { exit !ok }' && [ ! -e "$LANDDIR/current" ] && [ ! -e "$LANDDIR/current.tmp" ]
}
hist_has() { last_hist | awk -F'\t' -v s="$1" '{ for (i = 7; i <= NF; i++) if ($i ~ ("^" s "=[0-9]+$")) ok = 1 } END { exit !ok }'; }
main_is() { git fetch -q origin; [ "$(git rev-parse origin/main)" = "$1" ]; }
check() {
  N=$((N + 1))
  if eval "$2"; then echo "ok $N - $1"; else echo "not ok $N - $1"; FAILED=1; fi
}

# FORGE-011, #451: without cargo-nextest the landing stops at the start, with the install command,
# before it merges or builds anything (the fake cargo knows no `nextest`)
FAKEBIN="$TMP/fakebin"
mkdir -p "$FAKEBIN"
printf '#!/bin/sh\necho "error: no such command: nextest" >&2\nexit 101\n' > "$FAKEBIN/cargo"
chmod +x "$FAKEBIN/cargo"
branch nonextest n.txt n
before=$(git rev-parse origin/main)
PATH="$FAKEBIN:$PATH" expect "forge_011: a missing cargo-nextest stops the landing and names the install command" 2 "cargo install cargo-nextest --locked" nonextest
check "forge_011: a missing cargo-nextest lands nothing and starts no landing" "main_is $before && [ ! -e \"$TMP/work/.git/land/worktree\" ]"

export LAND_CHECK_CMD=true
# the lines-of-code report of DEV-045 is replaced too: it changes nothing unless a test says so
export LAND_REPORT_CMD=true
branch feature-b b.txt b
branch feature-c c.txt c
expect "two branches land in the order given" 0 "landed feature-c" feature-b feature-c
check "history: a landing has its steps, merge and push included" 'hist_is landed feature-c "" && hist_has merge && hist_has push'
check "main holds both, as merge commits" '[ "$(git show origin/main:b.txt)" = b ] && [ "$(git show origin/main:c.txt)" = c ] && git log -1 --format=%s origin/main | grep -Fq "Merge branch '"'"'feature-c'"'"' (landed locally, FORGE-011)"'
expect "a landed branch is not landed twice" 0 "already on origin/main" feature-b
check "history: nothing to land is recorded" 'hist_is nothing feature-b ""'

branch clash-1 a.txt left
git checkout -q -b clash-2 origin/main && printf 'right\n' > a.txt && git commit -qam "clash-2 (Refs #1)" && git checkout -q --detach
expect "the first of two clashing branches lands" 0 "landed clash-1" clash-1
before=$(git rev-parse origin/main)
expect "a conflict is refused and named" 1 "a.txt" clash-2
check "a conflict leaves main as it was" "main_is $before"
check "history: a conflict is recorded" 'hist_is conflict clash-2 "" && hist_has merge'
expect "an unknown name is an error landing" 1 "no branch or commit named nope0" nope0
check "history: an unknown name is recorded as error" 'hist_is error nope0 ""'
export LAND_CHECK_CMD='test -f "$(git rev-parse --path-format=absolute --git-common-dir)/land/current"'
branch running r.txt r
expect "current exists while the checks run" 0 "landed running" running
export LAND_CHECK_CMD=true
before=$(git rev-parse origin/main)

branch broken d.txt d
export LAND_CHECK_CMD=false
expect "a failing check pushes nothing" 1 "a check failed for broken" broken
check "main is unchanged after a failing check" "main_is $before"
check "history: a failing check names its step" 'hist_is failed broken false && hist_has false'
# a failing step stops the checks even when a later one passes (the bug that landed a red tree)
export LAND_CHECK_CMD=$'false\ntrue'
expect "a failing step is not hidden by a passing one after it" 1 "check failed: false" broken
check "main is unchanged after a failing step" "main_is $before"
check "history: only the steps that ran are recorded" 'hist_is failed broken false && ! hist_has true'
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
# the local main follows a landing (ADR-0018): its checkout is fast-forwarded when clean
git worktree add -q "$TMP/main-wt" main 2>/dev/null || git worktree add -q "$TMP/main-wt" -b main origin/main
git -C "$TMP/main-wt" reset -q --hard origin/main
git -C "$TMP/main-wt" reset -q --hard HEAD~1
branch follow f.txt f
expect "a landing fast-forwards the clean checkout of local main" 0 "local main is now" follow
check "the checkout of main has the landed file" '[ "$(cat "$TMP/main-wt/f.txt")" = f ] && [ "$(git rev-parse main)" = "$(git rev-parse origin/main)" ]'
branch dirty g.txt g
printf 'changed\n' > "$TMP/main-wt/a.txt"
mainwas=$(git rev-parse main)
expect "a checkout of main with changes is left and named" 0 "has uncommitted changes" dirty
check "local main did not move, the change is kept" '[ "$(git rev-parse main)" = "$mainwas" ] && [ "$(cat "$TMP/main-wt/a.txt")" = changed ]'
git -C "$TMP/main-wt" checkout -q -- a.txt
git -C "$TMP/main-wt" merge -q --ff-only origin/main
printf 'own\n' > "$TMP/main-wt/own.txt" && git -C "$TMP/main-wt" add own.txt && git -C "$TMP/main-wt" commit -qm "own (Refs #1)"
branch other h.txt h
mainwas=$(git rev-parse main)
expect "a local main with commits of its own is left and named" 0 "has commits that are not on origin/main" other
check "local main keeps its own commit" '[ "$(git rev-parse main)" = "$mainwas" ]'
git worktree remove --force "$TMP/main-wt"
git branch -q -f main origin/main~1
branch nocheckout i.txt i
expect "a local main that is not checked out anywhere moves as a ref" 0 "local main is now" nocheckout
check "the ref of local main is the landed commit" '[ "$(git rev-parse main)" = "$(git rev-parse origin/main)" ]'
check "no landing worktree is left behind" '! git worktree list | grep -Fq land/worktree'

# DEV-045: the lines-of-code report is made on the checked merge, committed on top of it and pushed with it
branch rep1 r1.txt r1
LAND_REPORT_CMD='mkdir -p docs && printf "report of the tree\n" > docs/loc-report.md' expect "dev_045: a changed report is made in the merged tree and committed" 0 "committed the lines-of-code report" rep1
check "dev_045: the report commit is on top of the merge, with its message, and is pushed" '[ "$(git log -1 --format=%s origin/main)" = "no-issue: lines-of-code report for rep1 (DEV-045)" ] && [ "$(git log -1 --format=%s origin/main^)" = "Merge branch '"'"'rep1'"'"' (landed locally, FORGE-011)" ] && [ "$(git log -1 --format=%P origin/main | wc -w)" = 1 ] && [ "$(git show origin/main:r1.txt)" = r1 ] && git show origin/main:docs/loc-report.md | grep -Fq "report of the tree"'
check "dev_045: the report is a step of the landing's record" 'hist_is landed rep1 "" && hist_has "lines-of-code report .DEV-045."'
check "dev_045: the local main follows to the report commit" '[ "$(git rev-parse main)" = "$(git rev-parse origin/main)" ]'
branch rep2 r2.txt r2
before=$(git rev-parse origin/main)
LAND_REPORT_CMD='mkdir -p docs && printf "report of the tree\n" > docs/loc-report.md' expect "dev_045: a report that did not change adds no commit" 0 "nothing to commit" rep2
check "dev_045: the landing of an unchanged report ends in the merge commit" '[ "$(git log -1 --format=%s origin/main)" = "Merge branch '"'"'rep2'"'"' (landed locally, FORGE-011)" ] && [ "$(git log -1 --format=%P origin/main | wc -w)" = 2 ] && git merge-base --is-ancestor "$before" origin/main'
branch rep3 r3.txt r3
before=$(git rev-parse origin/main)
LAND_REPORT_CMD='echo "no report today" >&2; exit 3' expect "dev_045: a failing report command fails the landing and is named" 1 "check failed: lines-of-code report (DEV-045)" rep3
check "dev_045: nothing is pushed when the report fails" "main_is $before"
check "dev_045: the failed landing is recorded with the report step" 'hist_is failed rep3 "lines-of-code report (DEV-045)"'
expect "dev_045: the same branch lands when the report works again" 0 "landed rep3" rep3

# DEV-046: the chart and its counts go into the same commit
branch rep1b r1b.txt r1b
LAND_REPORT_CMD='mkdir -p docs && printf "report 2\n" > docs/loc-report.md && printf "<svg/>\n" > docs/loc-history.svg && printf "# loc-history rules=1\n" > docs/loc-history.tsv' expect "dev_046: the report, the chart and the counts are committed together" 0 "committed the lines-of-code report" rep1b
check "dev_046: all three files are in the one report commit, which is pushed" '[ "$(git log -1 --format=%s origin/main)" = "no-issue: lines-of-code report for rep1b (DEV-045)" ] && [ "$(git show --name-only --format= origin/main | sort | tr "\n" " ")" = "docs/loc-history.svg docs/loc-history.tsv docs/loc-report.md " ]'
branch rep1c r1c.txt r1c
LAND_REPORT_CMD='printf "<svg/>\nmore\n" > docs/loc-history.svg' expect "dev_046: a changed chart alone makes a report commit" 0 "committed the lines-of-code report" rep1c
check "dev_046: a changed chart alone is committed on top of the merge" '[ "$(git show --name-only --format= origin/main)" = docs/loc-history.svg ]'

# the TeX Live mirror (DEV-025, #371): the check command prints what it sees of the variables
branch tex1 t1.txt t1
export LAND_CHECK_CMD='echo "seen install=[${SHALE_TEX_INSTALL_REPOSITORY-unset}] env=[${SHALE_TEX_ENV_REPOSITORY-unset}]"'
expect "dev_025: no mirror set: the default line is printed and neither variable is exported" 0 "seen install=[unset] env=[unset]" tex1
check "dev_025: the default line names the variable and the file" 'printf "%s" "$OUT" | grep -Fq "TeX Live: the default repository (no mirror set: SHALE_TEX_MIRROR or $LANDDIR/tex-mirror)"'
branch tex2 t2.txt t2
SHALE_TEX_MIRROR=https://texlive.example.test/tlnet/ expect "dev_025: SHALE_TEX_MIRROR is exported as both variables, the trailing / trimmed" 0 "seen install=[https://texlive.example.test/tlnet] env=[https://texlive.example.test/tlnet]" tex2
check "dev_025: the mirror line says where it is from" 'printf "%s" "$OUT" | grep -Fq "TeX Live: the mirror https://texlive.example.test/tlnet (from SHALE_TEX_MIRROR)"'
branch tex3 t3.txt t3
printf '# the homelab\n\n  http://file.example.test/tlnet  \nhttps://second.example.test/tlnet\n' > "$LANDDIR/tex-mirror"
expect "dev_025: the first line of the file that is no comment is used" 0 "seen install=[http://file.example.test/tlnet] env=[http://file.example.test/tlnet]" tex3
check "dev_025: the mirror line names the file" 'printf "%s" "$OUT" | grep -Fq "(from $LANDDIR/tex-mirror)"'
branch tex4 t4.txt t4
SHALE_TEX_MIRROR=https://var.example.test/tlnet expect "dev_025: the variable wins over the file" 0 "seen install=[https://var.example.test/tlnet]" tex4
branch tex5 t5.txt t5
SHALE_TEX_INSTALL_REPOSITORY=https://mine.example.test/x expect "dev_025: a SHALE_TEX_INSTALL_REPOSITORY set by the person is kept and said so" 0 "seen install=[https://mine.example.test/x] env=[http://file.example.test/tlnet]" tex5
check "dev_025: the kept variable is named" 'printf "%s" "$OUT" | grep -Fq "SHALE_TEX_INSTALL_REPOSITORY is set already, left as it is"'
branch tex6 t6.txt t6
SHALE_TEX_ENV_REPOSITORY=https://mine.example.test/y expect "dev_025: a SHALE_TEX_ENV_REPOSITORY set by the person is kept" 0 "seen install=[http://file.example.test/tlnet] env=[https://mine.example.test/y]" tex6
before=$(git rev-parse origin/main)
SHALE_TEX_MIRROR=ftp://nope.example.test/tlnet expect "dev_025: a mirror that is no http(s) URL exits 2" 2 "must start with https:// or http://" tex6
check "dev_025: a bad mirror lands nothing" "main_is $before"
printf 'texlive.example.test/tlnet\n' > "$LANDDIR/tex-mirror"
expect "dev_025: a bad mirror in the file exits 2 and names the file" 2 "(from $LANDDIR/tex-mirror) must start" tex6
rm -f "$LANDDIR/tex-mirror"
export LAND_CHECK_CMD=true

# the lock held by a release script (FORGE-011, #348): trusted only from the parent that names itself
LOCK="$(git rev-parse --path-format=absolute --git-common-dir)/land/lock"
branch locked j.txt j
N=$((N + 1))
if (exec 7>"$LOCK"; flock 7; SHALE_LAND_LOCK_HELD=bogus timeout 3 bash "$LAND" locked 8>&- 9>&- > "$TMP/wait.out" 2>&1; [ $? = 124 ] && grep -Fq "waiting" "$TMP/wait.out"); then
  echo "ok $N - a held lock is waited for, whatever SHALE_LAND_LOCK_HELD says"
else
  echo "not ok $N - a held lock is waited for, whatever SHALE_LAND_LOCK_HELD says"; FAILED=1
fi
N=$((N + 1))
if (exec 7>"$LOCK"; flock 7; SHALE_LAND_LOCK_HELD=$BASHPID bash "$LAND" locked 7>&- > "$TMP/held.out" 2>&1; [ $? = 0 ] && grep -Fq "landed locked" "$TMP/held.out"); then
  echo "ok $N - the parent that holds the lock lands without waiting for it"
else
  echo "not ok $N - the parent that holds the lock lands without waiting for it"; FAILED=1
fi

exit "$FAILED"

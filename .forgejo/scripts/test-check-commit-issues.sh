#!/bin/sh
# Tests .forgejo/scripts/check-commit-issues.sh on a throwaway git repository (spec FORGE-008).
#   sh .forgejo/scripts/test-check-commit-issues.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/check-commit-issues.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0

cd "$TMP" || exit 1
git init -q -b main .
git config user.email t@example.com
git config user.name test
git config commit.gpgsign false
commit() { git commit -q --allow-empty "$@"; }

# an old commit without an issue, then the cut-off, as in the real history
commit -m "Old work without any issue"
CUT=$(git rev-parse HEAD)
export SHALE_ISSUE_CHECK_CUTOFF="$CUT"
ZEROS=0000000000000000000000000000000000000000
UNKNOWN=1234567890123456789012345678901234567890

# expect <name> <pass|fail> <needle in the output> <base> <head>
expect() {
  N=$((N + 1))
  OUT=$(sh "$CHECK" "$4" "$5" 2>&1)
  CODE=$?
  if [ "$2" = pass ] && [ "$CODE" = 0 ] && printf '%s' "$OUT" | grep -Fq -- "$3"; then
    echo "ok $N - $1"
  elif [ "$2" = fail ] && [ "$CODE" != 0 ] && printf '%s' "$OUT" | grep -Fq -- "$3"; then
    echo "ok $N - $1"
  else
    echo "not ok $N - $1 (exit $CODE): $OUT"
    FAILED=1
  fi
}

expect "nothing after the cut-off passes" pass "0 commit(s)" "$CUT" "$CUT"

commit -m "Plain" -m "Refs #4"
commit -m "Closer" -m "Closes #5"
commit -m "Short" -m "see #6"
commit -m "Qualified" -m "Part of raba/shale#7"
commit -m "no-issue: fix a typo"
GOOD=$(git rev-parse HEAD)
expect "commits naming an issue or starting with no-issue: pass" pass "5 commit(s)" "$CUT" "$GOOD"

# a branch with a commit without an issue, merged: merge commits are not checked
git checkout -q -b side
commit -m "Side work without an issue"
git checkout -q main
git merge -q --no-ff side -m "Merge branch side"
MERGED=$(git rev-parse HEAD)
expect "a commit without an issue fails and is named" fail "names no issue: Side work without an issue" "$CUT" "$MERGED"
expect "the merge commit itself is not reported" pass "0 commit(s)" "$MERGED" "$MERGED"
commit -m "Mentions no-issue without the colon prefix"
expect "a commit that only mentions no-issue fails" fail "names no issue: Mentions no-issue" "$MERGED" main

# old commits are ignored; a new branch (all-zero base) and a vanished base check all new commits
expect "an all-zero base checks all new commits" fail "names no issue" "$ZEROS" main
expect "an empty base checks all new commits" fail "names no issue" "" main
expect "an unknown base (force push) checks all new commits" fail "is not in this checkout" "$UNKNOWN" main
expect "commits before the cut-off are not reported" pass "5 commit(s)" "$ZEROS" "$GOOD"

# a missing cut-off (shallow checkout) is an error, not a pass
N=$((N + 1))
if SHALE_ISSUE_CHECK_CUTOFF="$UNKNOWN" sh "$CHECK" "$CUT" main >/dev/null 2>&1; then
  echo "not ok $N - a missing cut-off commit must fail"
  FAILED=1
else
  echo "ok $N - a missing cut-off commit fails"
fi

[ "$FAILED" = 0 ] && echo "all $N checks passed"
exit "$FAILED"

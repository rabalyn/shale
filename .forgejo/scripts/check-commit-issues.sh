#!/bin/sh
# Fails when a commit names no issue (spec FORGE-008): every commit in BASE..HEAD, merge commits
# excepted, must contain an issue reference (`#N`, `Refs #N`, `Closes #N`, `raba/shale#N`) or its
# message must start with `no-issue:`.
#   sh .forgejo/scripts/check-commit-issues.sh <base> <head>
# <base> is the commit before the change: the `before` of a push, the base of a pull request.
# When it is empty, all zeros (a new branch) or not in the repository (a force push replaced it),
# every commit reachable from <head> is checked instead.
#
# Cut-off: commits that exist since before this rule are not checked. They are the commit below
# and its ancestors (it was `main` when the check was introduced). SHALE_ISSUE_CHECK_CUTOFF
# replaces it (the test does).
set -u
CUTOFF="${SHALE_ISSUE_CHECK_CUTOFF:-dbad07dfe05e8f34a137781c2d9b7277fd1c2b3e}"
BASE="${1:-}"
HEAD_REV="${2:-}"
[ -n "$HEAD_REV" ] || { echo "usage: check-commit-issues.sh <base> <head>" >&2; exit 2; }

git rev-parse -q --verify "$CUTOFF^{commit}" >/dev/null || {
  echo "error: the cut-off commit $CUTOFF is not in this checkout; fetch the full history (git fetch --unshallow)" >&2
  exit 2
}
git rev-parse -q --verify "$HEAD_REV^{commit}" >/dev/null || { echo "error: $HEAD_REV is not a commit here" >&2; exit 2; }

case "$BASE" in *[!0]*) ;; *) BASE="" ;; esac
if [ -n "$BASE" ] && ! git rev-parse -q --verify "$BASE^{commit}" >/dev/null; then
  echo "note: $BASE is not in this checkout (new or force-pushed branch), checking everything up to $HEAD_REV"
  BASE=""
fi

BAD=0
CHECKED=0
set -- "$HEAD_REV" --not "$CUTOFF"
[ -z "$BASE" ] || set -- "$@" "$BASE"
for c in $(git rev-list --no-merges "$@"); do
  CHECKED=$((CHECKED + 1))
  MSG=$(git log -1 --format=%B "$c")
  case "$MSG" in "no-issue:"*) continue ;; esac
  printf '%s\n' "$MSG" | grep -Eq '#[0-9]+' && continue
  echo "error: commit $(git rev-parse --short "$c") names no issue: $(git log -1 --format=%s "$c")" >&2
  BAD=1
done
if [ "$BAD" = 1 ]; then
  echo "Name the issue in each commit (\"Refs #N\", \"Closes #N\", \"raba/shale#N\") or start the message with \"no-issue:\" (typo or formatting fixes only)." >&2
  exit 1
fi
echo "Commit check passed ($CHECKED commit(s) checked)."

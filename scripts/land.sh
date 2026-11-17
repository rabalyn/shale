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
# `-D warnings` for Linux, and for macOS and Windows (`scripts/cross-check.sh`), `cargo test` (also `cargo test --release` with --release), and the real-server
# suites of tests/caldav-env and tests/mail-env and the LuaLaTeX suite of tests/tex-env when the
# change touches the CalDAV, mail or TeX export code or the environments (`.forgejo/scripts/env-tests.sh wanted`); --no-env-tests skips them and
# says so. A merge conflict is not resolved here: merge `origin/main` into the branch, resolve
# it there and run land.sh again.
#
# The worktree and the build folder stay in `<git dir>/land/`, so the next landing builds only
# what changed. Environment, for the test of this script: LAND_REMOTE (default origin),
# LAND_CHECK_CMD (a shell command run in the worktree instead of the checks above).
set -euo pipefail

REMOTE=${LAND_REMOTE:-origin}
RELEASE=no
ENV_TESTS=yes
BRANCHES=()
for a in "$@"; do
  case $a in
    --release) RELEASE=yes ;;
    --no-env-tests) ENV_TESTS=no ;;
    -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "land.sh: unknown option $a" >&2; exit 2 ;;
    *) BRANCHES+=("$a") ;;
  esac
done
if [ ${#BRANCHES[@]} -eq 0 ]; then
  echo "usage: scripts/land.sh [--release] [--no-env-tests] <branch>…" >&2
  exit 2
fi

COMMON=$(git rev-parse --path-format=absolute --git-common-dir)
HOME_DIR="$COMMON/land"
WT="$HOME_DIR/worktree"
mkdir -p "$HOME_DIR"
export CARGO_TARGET_DIR="${LAND_TARGET_DIR:-$HOME_DIR/target}"

exec 9>"$HOME_DIR/lock"
if ! flock -n 9; then
  echo "Another landing is running; waiting for it…"
  flock 9
fi

remove_worktree() {
  if [ -e "$WT" ]; then git worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; fi
  git worktree prune
}
trap remove_worktree EXIT

# One check: its name, then the command. A failing one ends the checks with its name. Every
# step returns its own status, so this holds without `set -e`, which bash ignores in a function
# called from `if` (a failing `cargo test` once let a branch land).
step() {
  local name=$1
  shift
  echo "-- $name"
  if ! "$@"; then
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
  # code used only from Linux-only `cfg` code is dead on macOS and Windows (BUILD-004, DEV-015)
  step "macOS and Windows clippy (scripts/cross-check.sh)" sh scripts/cross-check.sh || return 1
  step "cargo test" cargo test -q --locked || return 1
  if [ "$RELEASE" = yes ]; then
    step "cargo test --release" cargo test -q --release --locked || return 1
  fi
  if [ "$(sh .forgejo/scripts/env-tests.sh wanted "$base" HEAD)" = yes ]; then
    if [ "$ENV_TESTS" = no ]; then
      echo "-- SKIPPED (--no-env-tests): the change touches CalDAV, mail or TeX export, the real-server suites did not run"
    else
      step "real-server suites (podman)" sh .forgejo/scripts/env-tests.sh run || return 1
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

for b in "${BRANCHES[@]}"; do
  echo "== $b"
  git fetch -q "$REMOTE"
  if ! ref=$(resolve "$b"); then
    echo "land.sh: no branch or commit named $b" >&2
    exit 1
  fi
  base=$(git rev-parse "refs/remotes/$REMOTE/main")
  if git merge-base --is-ancestor "$ref" "$base"; then
    echo "already on $REMOTE/main, nothing to land"
    continue
  fi
  remove_worktree
  git worktree add -q --detach "$WT" "$base"
  if ! git -C "$WT" merge -q --no-ff --no-edit -m "Merge branch '$b' (landed locally, FORGE-011)" "$ref" >/dev/null 2>&1; then
    echo "land.sh: $b conflicts with $REMOTE/main in:" >&2
    git -C "$WT" diff --name-only --diff-filter=U | sed 's/^/  /' >&2
    echo "Merge $REMOTE/main into $b, resolve the conflict there and run land.sh again. $REMOTE/main is unchanged." >&2
    exit 1
  fi
  if ! (cd "$WT" && checks "$base"); then
    echo "land.sh: a check failed for $b; $REMOTE/main is unchanged." >&2
    exit 1
  fi
  if ! git -C "$WT" push -q "$REMOTE" HEAD:main; then
    echo "land.sh: $REMOTE/main moved while $b was checked (pushed by hand?); run land.sh again." >&2
    exit 1
  fi
  echo "landed $b as $(git -C "$WT" rev-parse --short HEAD) on $REMOTE/main"
done

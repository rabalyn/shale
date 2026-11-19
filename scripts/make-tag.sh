#!/bin/bash
# Makes a check tag (FORGE-013, ADR-0018): a pre-release `vX.Y.Z-rc.N` of the current
# `origin/main`, for when the work is done for now. Forgejo's CI runs for it once; when it is
# green, its `mirror` job pushes `main` to GitHub, which builds nothing (the tag stays on Forgejo).
#
#   scripts/make-tag.sh [--dry-run] [--issue N] [patch|minor|major|X.Y.Z-rc.N]
#
# Without a version it takes the next `rc` of the pre-release in Cargo.toml of `origin/main`
# (0.4.0-rc.1 → 0.4.0-rc.2); `patch`, `minor` or `major` raise the last release (0.3.0 → minor →
# 0.4.0-rc.1, or the next rc when Cargo.toml has a pre-release of that version already).
# It stops with "the changelog needs an update first" when CHANGELOG.md and docs/changelog-pending/
# did not change since the previous tag. It bumps Cargo.toml and Cargo.lock on a branch
# `release/vX.Y.Z-rc.N`, merges docs/changelog-pending/ into [Unreleased], commits naming the issue
# "Release X.Y.Z" (made with gh when there is none; --issue N names another), lands it with
# `scripts/land.sh --release`, checks the landed merge (BUILD-013), tags it and pushes only the tag
# to `origin`. The landing lock is held for the whole run (a landing meanwhile waits); an
# `X.Y.Z-rc.N` that is on `origin/main` already without a tag (the check failed) is tagged by a rerun.
# --dry-run prints the tag and stops. A release is scripts/make-release.sh.
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/release-lib.sh"

DRY=no
ISSUE=""
WANT=""
while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) DRY=yes ;;
    --issue) shift; ISSUE=${1:-} ;;
    -h|--help) sed -n '2,/^set -euo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$WANT" ] || die "one version only"; WANT=$1 ;;
  esac
  shift
done

# the landing lock for the whole run, before `origin/main` is read (#348); a dry run changes nothing
[ "$DRY" = yes ] || take_land_lock
preflight
CUR=$(version_at "refs/remotes/$REMOTE/main")
LAST=$(last_release)
case $WANT in
  "")
    is_pre "$CUR" || die "Cargo.toml has the release $CUR; say patch, minor or major for the first check tag of the next version"
    [[ $CUR =~ -rc\.([0-9]+)$ ]] || die "Cargo.toml has $CUR, which does not end in -rc.N; name the version"
    V="${CUR%.*}.$((BASH_REMATCH[1] + 1))"
    ;;
  patch|minor|major)
    T=$(bump "$WANT" "${LAST:-$(core "$CUR")}")
    if is_pre "$CUR" && [ "$(core "$CUR")" = "$T" ] && [[ $CUR =~ -rc\.([0-9]+)$ ]]; then
      V="$T-rc.$((BASH_REMATCH[1] + 1))"
    else
      V="$T-rc.1"
    fi
    ;;
  *) V=${WANT#v} ;;
esac
echo "origin/main has $CUR; the last release is ${LAST:-none}"
tag_and_push check "$V" "$ISSUE" "$DRY"

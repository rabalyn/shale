#!/bin/bash
# Makes a release (FORGE-014, ADR-0018): the tag `vX.Y.Z` of the current `origin/main`. Forgejo's
# CI checks it; when it is green, its `mirror` job pushes `main` and the tag to GitHub, which
# builds and publishes the release (BUILD-012, BUILD-016).
#
#   scripts/make-release.sh [--dry-run] [--issue N] [patch|minor|major|X.Y.Z]
#
# Without a version it releases the pre-release in Cargo.toml of `origin/main` (0.4.0-rc.2 →
# 0.4.0, the version the check tags of scripts/make-tag.sh led up to); `patch`, `minor` or
# `major` raise the last release. On a branch `release/vX.Y.Z` it bumps Cargo.toml and
# Cargo.lock, turns `## [Unreleased]` and docs/changelog-pending/*.md into the section
# `## [X.Y.Z] - <date>`, runs the release check (BUILD-013), commits naming the issue
# "Release X.Y.Z" (made with gh when there is none; --issue N names another), lands it with
# `scripts/land.sh --release`, tags the landed merge and pushes the tag to `origin`.
# --dry-run prints the tag and stops.
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/release-lib.sh"

DRY=no
ISSUE=""
WANT=""
while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) DRY=yes ;;
    --issue) shift; ISSUE=${1:-} ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$WANT" ] || die "one version only"; WANT=$1 ;;
  esac
  shift
done

preflight
CUR=$(version_at "refs/remotes/$REMOTE/main")
LAST=$(last_release)
case $WANT in
  "")
    is_pre "$CUR" || die "Cargo.toml has the release $CUR already; say patch, minor or major"
    V=$(core "$CUR")
    ;;
  patch|minor|major) V=$(bump "$WANT" "${LAST:-$(core "$CUR")}") ;;
  *) V=${WANT#v} ;;
esac
echo "origin/main has $CUR; the last release is ${LAST:-none}"
tag_and_push release "$V" "$ISSUE" "$DRY"

#!/bin/bash
# Makes a release (FORGE-014, ADR-0018): the tag `vX.Y.Z` of the current `origin/main`. Forgejo's
# CI checks it; when it is green, its `mirror` job pushes `main` and the tag to GitHub, which
# builds and publishes the release (BUILD-012, BUILD-016).
#
#   scripts/make-release.sh [--dry-run] [--issue N] [--skip-bench] [--allow-perf-regression] [--ignore-github-check] [patch|minor|major|X.Y.Z]
#
# Without a version it releases the pre-release in Cargo.toml of `origin/main` (0.4.0-rc.2 →
# 0.4.0, the version the check tags of scripts/make-tag.sh led up to); `patch`, `minor` or
# `major` raise the last release. On a branch `release/vX.Y.Z` it bumps Cargo.toml and
# Cargo.lock, turns `## [Unreleased]` and docs/changelog-pending/*.md into the section
# `## [X.Y.Z] - <date>`, runs the release check (BUILD-013), commits naming the issue
# "Release X.Y.Z" (made with gh when there is none; --issue N names another), lands it with
# `scripts/land.sh --release`, checks the landed merge (BUILD-013, and docs/changelog-pending/
# empty), tags it and pushes the tag to `origin`. The landing lock is held for the whole run (a
# landing meanwhile waits). When the check of the landed merge fails nothing is tagged: add the
# missing entry in a branch, land it, and run `make-release.sh X.Y.Z` again, which tags `origin/main`.
# Before the commit it runs scripts/bench-release.sh (BUILD-034): the full and the worst benchmark
# profiles, stored under docs/perf/ with their comparison against the last release; a regression
# stops the release unless --allow-perf-regression, but a flagged time regression is first measured again
# alone, interleaved, and counts only if it shows again (#408; its numbers are commented on the release
# issue); --skip-bench leaves the step out.
# It also makes the binary size report (scripts/size-report.sh, BUILD-045: docs/size-report.md, its charts and
# history) in the release worktree, so it is in the release commit; a dry run does not build it.
# Before anything is changed it looks at GitHub's workflow main-check (BUILD-039, ADR-0025, #453):
# when its latest run on `main` is red or still running (macOS or Windows fail there at the check tag,
# before a release is made) the release stops; --ignore-github-check skips that, and a gh that
# cannot answer stops it too. A dry run reports it.
# --dry-run prints the tag and stops. Without entries in [Unreleased] or docs/changelog-pending/
# it stops with "the changelog needs an update first".
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/release-lib.sh"

DRY=no
BENCH_SKIP=no
BENCH_ALLOW=
IGNORE_GH=no
ISSUE=""
WANT=""
while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) DRY=yes ;;
    --issue) shift; ISSUE=${1:-} ;;
    --skip-bench) BENCH_SKIP=yes ;;
    --allow-perf-regression) BENCH_ALLOW=yes ;;
    --ignore-github-check) IGNORE_GH=yes ;;
    -h|--help) sed -n '2,/^set -euo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$WANT" ] || die "one version only"; WANT=$1 ;;
  esac
  shift
done

# the landing lock for the whole run, before `origin/main` is read (#348); a dry run changes nothing
[ "$DRY" = yes ] || take_land_lock
preflight
RELEASE_IGNORE_GH_CHECK=$IGNORE_GH github_main_check "$DRY"
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

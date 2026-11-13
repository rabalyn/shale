#!/bin/sh
# Makes a release: tags the current commit with the version from Cargo.toml and pushes it.
# GitHub then builds it and creates the release page with that version's CHANGELOG.md notes.
#
# Before running: raise `version` in Cargo.toml (semver), run `cargo build` once (updates
# Cargo.lock), add a "## [x.y.z] - date" section to CHANGELOG.md, commit everything.
#   sh make-release.sh
set -e
cd "$(dirname "$0")"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TAG="v$VERSION"
sh .github/release-notes.sh "$VERSION" >/dev/null
git diff --quiet && git diff --cached --quiet || { echo "There are uncommitted changes; commit them first."; exit 1; }
grep -q "^name = \"shale\"" Cargo.lock && grep -A1 '^name = "shale"' Cargo.lock | grep -q "version = \"$VERSION\"" || { echo "Cargo.lock still has the old version; run cargo build and commit."; exit 1; }
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag $TAG exists already (pointing at $(git rev-parse --short "$TAG^{commit}"))."
else
  git tag -a "$TAG" -m "Shale $VERSION"
fi
git push origin HEAD "$TAG"
echo "Pushed $TAG. The release page appears when the build is done:"
echo "  $(git remote get-url origin | sed 's/\.git$//' | sed 's#git@github.com:#https://github.com/#')/releases/tag/$TAG"

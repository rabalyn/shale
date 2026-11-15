#!/bin/sh
# Makes a release: tags the current commit with the version from Cargo.toml and pushes the
# commit and the tag to both servers: the homelab Forgejo (remote `origin`, where CI runs) and
# GitHub (remote `github`), which builds it again and creates the release page with that
# version's CHANGELOG.md notes and the download's .sha256 file. Other remote names:
#   FORGEJO_REMOTE=… GITHUB_REMOTE=… sh make-release.sh
#
# Before running: raise `version` in Cargo.toml (semver), run `cargo build` once (updates
# Cargo.lock), add a "## [x.y.z] - date" section to CHANGELOG.md, commit everything.
#   sh make-release.sh
set -e
cd "$(dirname "$0")"
FORGEJO_REMOTE="${FORGEJO_REMOTE:-origin}"
GITHUB_REMOTE="${GITHUB_REMOTE:-github}"
for r in "$FORGEJO_REMOTE" "$GITHUB_REMOTE"; do
  git remote get-url "$r" >/dev/null 2>&1 || { echo "There is no git remote \"$r\" (see the top of this script)."; exit 1; }
done
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
git push "$FORGEJO_REMOTE" HEAD "$TAG"
git push "$GITHUB_REMOTE" HEAD "$TAG"
echo "Pushed $TAG to $FORGEJO_REMOTE and $GITHUB_REMOTE. The release page appears when the GitHub build is done:"
echo "  $(git remote get-url "$GITHUB_REMOTE" | sed 's/\.git$//' | sed 's#git@github.com:#https://github.com/#')/releases/tag/$TAG"

#!/bin/sh
# Puts this project on GitHub as a public repository and starts the first build.
# Run it once from this folder on a computer where git and the GitHub CLI (gh) are installed
# and signed in (gh auth login):   sh publish-to-github.sh
# Later changes: git add -A && git commit -m "…" && git push
set -e
OWNER="${OWNER:-rabalyn}"
REPO="${REPO:-shale}"
cd "$(dirname "$0")"

command -v git >/dev/null || { echo "git is not installed"; exit 1; }
command -v gh >/dev/null || { echo "the GitHub CLI (gh) is not installed"; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "sign in first: gh auth login"; exit 1; }

if [ ! -d .git ]; then
  git init -b main
fi
git add -A
if ! git diff --cached --quiet; then
  git commit -m "Shale: native notes app with links, drawings, tasks, calendar and import

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
fi

if git remote get-url origin >/dev/null 2>&1; then
  git push -u origin main
else
  gh repo create "$OWNER/$REPO" --public --source . --remote origin --push \
    --description "Native Rust notes app for Linux: markdown vault, wiki links, drawings, tasks, CalDAV calendar, e-mail import"
fi

echo
echo "Done. The build runs here:   https://github.com/$OWNER/$REPO/actions"
echo "Download the newest build:   https://github.com/$OWNER/$REPO/releases/tag/latest"

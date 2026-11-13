#!/bin/sh
# Prints the CHANGELOG.md section of one version (without its heading), for the release page.
# Fails when the changelog has no (or an empty) section for it.
#   sh .github/release-notes.sh 0.2.0
set -e
VERSION="${1#v}"
[ -n "$VERSION" ] || { echo "usage: release-notes.sh <version>" >&2; exit 2; }
FILE="${2:-$(dirname "$0")/../CHANGELOG.md}"
NOTES=$(awk -v v="$VERSION" '
  /^## \[/ { if (on) exit; if (index($0, "## [" v "]") == 1) { on = 1; next } }
  /^\[[^]]+\]: / { if (on) exit }
  on { print }
' "$FILE" | sed -e '/./,$!d')
if [ -z "$(printf '%s' "$NOTES" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no notes for version $VERSION" >&2
  exit 1
fi
printf '%s\n' "$NOTES"

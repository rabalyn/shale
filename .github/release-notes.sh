#!/bin/sh
# Prints the release page text of one version: the AI notice (.github/ai-notice.md, BUILD-014)
# and then the CHANGELOG.md section of the version (without its heading).
# Fails when the changelog has no (or an empty) section for it.
#   sh .github/release-notes.sh 0.2.0
set -e
VERSION="${1#v}"
[ -n "$VERSION" ] || { echo "usage: release-notes.sh <version>" >&2; exit 2; }
FILE="${2:-$(dirname "$0")/../CHANGELOG.md}"
NOTICE="$(dirname "$0")/ai-notice.md"
NOTES=$(awk -v v="$VERSION" '
  /^## \[/ { if (on) exit; if (index($0, "## [" v "]") == 1) { on = 1; next } }
  /^\[[^]]+\]: / { if (on) exit }
  on { print }
' "$FILE" | sed -e '/./,$!d')
if [ -z "$(printf '%s' "$NOTES" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no notes for version $VERSION" >&2
  exit 1
fi
cat "$NOTICE"
printf '\n%s\n' "$NOTES"

#!/bin/sh
# Checks the AI-development notice (spec BUILD-015): the first block of the README after the
# title and the badge is a "> [!WARNING]" alert whose next line is the headline of
# .github/ai-notice.md, and the release notes start with .github/ai-notice.md.
#   sh .github/check-ai-notice.sh [README.md]
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
README="${1:-$HERE/../README.md}"
NOTICE="$HERE/ai-notice.md"
FAILED=0

# the headline: the bold sentence that starts the notice's first paragraph
HEADLINE=$(grep -o -m1 '^\*\*[^*]*\*\*' "$NOTICE")
if [ -z "$HEADLINE" ]; then
  echo "ai-notice.md has no bold headline sentence" >&2
  exit 1
fi

# the first two lines after the title, the badges and blank lines
FIRST=$(awk 'NR == 1 && /^# / { next } /^\[!\[/ { next } /^[[:space:]]*$/ { next } { print; n++ } n == 2 { exit }' "$README")
if [ "$FIRST" != "$(printf '> [!WARNING]\n> %s' "$HEADLINE")" ]; then
  echo "README.md does not start with the AI notice (> [!WARNING] and > $HEADLINE)" >&2
  FAILED=1
fi

# release-notes.sh prints the notice before the changelog section
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf '# Changelog\n\n## [0.0.1] - 2026-01-01\n\n- a change\n' > "$TMP/CHANGELOG.md"
if ! sh "$HERE/release-notes.sh" 0.0.1 "$TMP/CHANGELOG.md" > "$TMP/notes.md"; then
  echo "release-notes.sh failed" >&2
  FAILED=1
elif [ "$(head -n "$(wc -l < "$NOTICE")" "$TMP/notes.md")" != "$(cat "$NOTICE")" ]; then
  echo "the release notes do not start with .github/ai-notice.md" >&2
  FAILED=1
fi
exit "$FAILED"

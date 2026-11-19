#!/bin/sh
# Prints the release page text of one version: the AI notice (.github/ai-notice.md, BUILD-015)
# and then the CHANGELOG.md section of the version (without its heading).
# Fails when the changelog has no (or an empty) section for it.
#   sh .github/release-notes.sh 0.2.0
# When SHALE_PREV_PUBLISHED names the last release that has a published release page (a tag such
# as v0.6.0, BUILD-013), the page also covers every version released since: all the sections
# greater than it, up to and including the version, newest first, each under its own
# "## x.y.z - date" heading, so a reader sees which version brought what (versions that never
# got a page of their own are not lost). With no section between, the output is the single
# section as above. Unset or empty: the one section. The variable is read here (not a third
# argument) so the workflows and check-release.sh set it once and cannot disagree.
set -e
VERSION="${1#v}"
[ -n "$VERSION" ] || { echo "usage: release-notes.sh <version>" >&2; exit 2; }
FILE="${2:-$(dirname "$0")/../CHANGELOG.md}"
NOTICE="$(dirname "$0")/ai-notice.md"
SINCE="${SHALE_PREV_PUBLISHED:-}"
SINCE="${SINCE#v}"

# section <version>: the lines of its section, without the heading and leading blank lines
section() {
  awk -v v="$1" '
    /^## \[/ { if (on) exit; if (index($0, "## [" v "]") == 1) { on = 1; next } }
    /^\[[^]]+\]: / { if (on) exit }
    on { print }
  ' "$FILE" | sed -e '/./,$!d'
}
blank() { [ -z "$(printf '%s' "$1" | tr -d '[:space:]')" ]; }

NOTES=$(section "$VERSION")
if blank "$NOTES"; then
  echo "CHANGELOG.md has no notes for version $VERSION" >&2
  exit 1
fi

# the older versions the page also covers: sections with SINCE < version < VERSION, newest first
# (the order of the file)
OLDER=""
if [ -n "$SINCE" ]; then
  for v in $(sed -n 's/^## \[\([0-9][^]]*\)\].*/\1/p' "$FILE"); do
    [ "$v" != "$VERSION" ] || continue
    [ "$(printf '%s\n%s\n' "$v" "$SINCE" | sort -V | tail -n 1)" = "$v" ] && [ "$v" != "$SINCE" ] || continue
    [ "$(printf '%s\n%s\n' "$v" "$VERSION" | sort -V | tail -n 1)" = "$VERSION" ] || continue
    OLDER="$OLDER $v"
  done
fi

cat "$NOTICE"
if [ -z "$OLDER" ]; then
  printf '\n%s\n' "$NOTES"
  exit 0
fi
# the heading text after "## [x.y.z]" (" - date"), for the page heading
heading() {
  d=$(sed -n "s/^## \[$1\][ -]*//p" "$FILE" | head -n 1)
  if [ -n "$d" ]; then printf '## %s - %s\n' "$1" "$d"; else printf '## %s\n' "$1"; fi
}
printf '\n'
heading "$VERSION"
printf '\n%s\n' "$NOTES"
for v in $OLDER; do
  body=$(section "$v")
  blank "$body" && continue
  printf '\n'
  heading "$v"
  printf '\n%s\n' "$body"
done

#!/bin/sh
# Tests .github/check-ai-notice.sh on throwaway READMEs (spec BUILD-015).
#   sh .github/test-check-ai-notice.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/check-ai-notice.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
HEAD='> **Shale is built entirely by AI coding agents.**'
BADGE='[![build](https://example.com/b.svg)](https://example.com)'

# expect <name> <pass|fail> <text the error output must contain> <README text>
expect() {
  N=$((N + 1))
  printf '%s\n' "$4" > "$TMP/README.md"
  OUT=$(sh "$CHECK" "$TMP/README.md" 2>&1)
  CODE=$?
  if [ "$2" = pass ] && [ "$CODE" = 0 ]; then
    echo "ok $N - $1"
  elif [ "$2" = fail ] && [ "$CODE" != 0 ] && printf '%s' "$OUT" | grep -Fq -- "$3"; then
    echo "ok $N - $1"
  else
    echo "not ok $N - $1 (exit $CODE): $OUT"
    FAILED=1
  fi
}

expect "the real README passes" pass "" "$(cat "$HERE/../README.md")"
expect "notice right after title and badge passes" pass "" \
  "$(printf '# Shale\n\n%s\n\n> [!WARNING]\n%s\n>\n> more\n\nText.' "$BADGE" "$HEAD")"
expect "notice without a badge passes" pass "" \
  "$(printf '# Shale\n\n> [!WARNING]\n%s\n' "$HEAD")"
expect "no notice fails" fail "does not start with the AI notice" \
  "$(printf '# Shale\n\n%s\n\nA notes app.' "$BADGE")"
expect "notice below other text fails" fail "does not start with the AI notice" \
  "$(printf '# Shale\n\nA notes app.\n\n> [!WARNING]\n%s\n' "$HEAD")"
expect "a NOTE alert instead of WARNING fails" fail "does not start with the AI notice" \
  "$(printf '# Shale\n\n> [!NOTE]\n%s\n' "$HEAD")"
expect "a different headline fails" fail "does not start with the AI notice" \
  "$(printf '# Shale\n\n> [!WARNING]\n> **Made with some help.**\n')"

[ "$FAILED" = 0 ] && echo "all $N checks passed"
exit "$FAILED"

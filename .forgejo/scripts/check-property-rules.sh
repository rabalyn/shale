#!/bin/sh
# Fails when a property test of a spec rule changes without its rule (spec DEV-043): in the commits
# BASE..HEAD, a diff that changes or removes the body of a property test must also change the spec
# row of the rule it checks (a line of that row in `docs/spec/`), or some commit message in the
# range must carry a line `rule-unchanged: <ID> <why>` (a rename, a faster generator, a new case).
# A property that is only added needs nothing. Otherwise a failing property could be made to pass
# by weakening it, and nothing would notice.
#   sh .forgejo/scripts/check-property-rules.sh <base> <head>
#
# What counts as a property, found in the OLD version of each changed or deleted `.rs` file that
# mentions `proptest` (a renamed file counts as deleted and added):
#   - a `fn` inside a `proptest! { … }` block, or a `fn` whose body uses `prop_assert`, `proptest!`
#     or `TestRunner`, and
#   - that names a spec ID: its name starts with one (`vault_018_…` is VAULT-018, and
#     `forge_013_forge_014_…` is both rules), or the lines above it (its attributes and comments)
#     hold `// spec: ID` (more than one ID may follow).
# Its span is the `fn` line (and the attribute and comment lines directly above it) up to the
# closing brace that balances its first `{`; braces are counted after the text of strings, chars
# and `//` comments is dropped (a raw string with a brace in it can confuse that: that only
# costs a `rule-unchanged:` line). It is changed when a hunk of `git diff -U0` touches a line of
# its span, or inserts lines inside it. It is guarded as soon as any line of it is touched, also
# whitespace, so the check leans to a false alarm (one `rule-unchanged:` line) over a miss.
set -u
BASE="${1:-}"
HEAD_REV="${2:-}"
[ -n "$HEAD_REV" ] || { echo "usage: check-property-rules.sh <base> <head>" >&2; exit 2; }
git rev-parse -q --verify "$BASE^{commit}" >/dev/null || { echo "error: $BASE is not a commit here" >&2; exit 2; }
git rev-parse -q --verify "$HEAD_REV^{commit}" >/dev/null || { echo "error: $HEAD_REV is not a commit here" >&2; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
HITS="$TMP/hits"
: > "$HITS"

# awk: print "<first line> <last line> <fn name> <ID>" for each guarded property of the file on stdin
SPANS='
{ L[NR] = $0 }
END {
  depth = 0; ptd = -1; infn = 0
  for (i = 1; i <= NR; i++) {
    code = L[i]
    gsub(/"([^"\\]|\\.)*"/, "\"\"", code)
    gsub(/\047(\\.|[^\\\047])\047/, "", code)
    sub(/\/\/.*/, "", code)
    nopen = gsub(/\{/, "{", code); nclose = gsub(/\}/, "}", code)
    if (!infn && ptd < 0 && code ~ /proptest!/) ptd = depth
    if (!infn && match(code, /(^|[^A-Za-z0-9_])fn[ \t]+[A-Za-z_][A-Za-z0-9_]*/)) {
      name = substr(code, RSTART, RLENGTH); sub(/^.*fn[ \t]+/, "", name)
      infn = 1; opened = 0; base = depth; inpt = (ptd >= 0 && depth > ptd); flag = 0; nids = 0
      j = i - 1
      while (j >= 1 && L[j] ~ /^[ \t]*(#\[|\/\/)/) j--
      first = j + 1
      for (k = first; k < i; k++) if (L[k] ~ /\/\/[ \t]*spec:/) {
        t = L[k]; sub(/^.*spec:/, "", t)
        while (match(t, /[A-Z][A-Z]*-[0-9][0-9]*/)) { ids[++nids] = substr(t, RSTART, RLENGTH); t = substr(t, RSTART + RLENGTH) }
      }
      rest = name
      while (match(rest, /^[a-z][a-z]*_[0-9][0-9]*(_|$)/)) {
        id = substr(rest, 1, RLENGTH); sub(/_$/, "", id); rest = substr(rest, RLENGTH + 1)
        n = split(id, p, "_"); ids[++nids] = toupper(p[1]) "-" p[2]
      }
    }
    if (infn) {
      if (code ~ /prop_assert|proptest!|TestRunner/) flag = 1
      if (nopen > 0) opened = 1
    }
    depth += nopen - nclose
    if (ptd >= 0 && !infn && depth <= ptd && nclose > 0) ptd = -1
    if (infn && opened && depth <= base) {
      if (inpt || flag) for (k = 1; k <= nids; k++) print first, i, name, ids[k]
      infn = 0
      if (ptd >= 0 && depth <= ptd) ptd = -1
    }
  }
}'

git diff --name-only --no-renames --diff-filter=MD "$BASE" "$HEAD_REV" -- '*.rs' > "$TMP/files"
while IFS= read -r f; do
  git show "$BASE:$f" > "$TMP/old.rs" 2>/dev/null || continue
  grep -q proptest "$TMP/old.rs" || continue
  awk "$SPANS" "$TMP/old.rs" > "$TMP/spans"
  [ -s "$TMP/spans" ] || continue
  git diff -U0 --no-renames "$BASE" "$HEAD_REV" -- "$f" |
    sed -n 's/^@@ -\([0-9][0-9]*\)\(,\([0-9][0-9]*\)\)\{0,1\} .*/\1 \3/p' > "$TMP/hunks"
  while read -r a b; do
    b=${b:-1}
    while read -r s e name id; do
      if [ "$b" = 0 ]; then
        [ "$a" -ge "$s" ] && [ "$a" -lt "$e" ] || continue   # lines inserted after line a, inside the span
      else
        [ "$a" -le "$e" ] && [ $((a + b - 1)) -ge "$s" ] || continue
      fi
      echo "$f $name $id" >> "$HITS"
    done < "$TMP/spans"
  done < "$TMP/hunks"
done < "$TMP/files"

sort -u "$HITS" -o "$HITS"
if [ ! -s "$HITS" ]; then
  echo "Property check passed (no property test of a spec rule changed or removed)."
  exit 0
fi

git diff -U0 "$BASE" "$HEAD_REV" -- docs/spec > "$TMP/spec.diff"
git log --format=%B "$BASE..$HEAD_REV" > "$TMP/messages"
BAD=0
while read -r f name id; do
  grep -Eq "^[-+]\| $id \|" "$TMP/spec.diff" && continue
  grep -Eq "^rule-unchanged: $id [^ ]" "$TMP/messages" && continue
  echo "error: property $name ($f) of $id changed or was removed, but the row $id in docs/spec/ did not change and no commit says why" >&2
  BAD=1
done < "$HITS"
if [ "$BAD" = 1 ]; then
  echo "A property test is the rule written once, so changing it changes the rule: change the spec row of the ID too, or, when the rule stays as it is (a rename, a faster generator, a new case), put a line \"rule-unchanged: <ID> <why>\" in a commit message of the change." >&2
  exit 1
fi
echo "Property check passed ($(wc -l < "$HITS" | tr -d ' ') changed property test(s), each with its rule changed or a rule-unchanged line)."

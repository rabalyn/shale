#!/bin/sh
# Checks that a version may be released (spec BUILD-013). Run in the repository to check (the
# current directory), before tagging (make-release.sh) and for a `v*` tag in CI (Forgejo and
# GitHub; the checkout needs the tags and the history since the previous tag).
#   sh .github/check-release.sh 0.4.0
# Fails with a message per problem unless:
#   - the version is valid semver and greater than the previous `v*` tag (the highest semver
#     `v*` tag that is not on HEAD, so a tag being built is not its own predecessor);
#   - Cargo.toml and the `shale` package in Cargo.lock have this version;
#   - the CHANGELOG.md section of the version is not empty, and mentions (as `#N` or
#     `raba/shale#N`) every issue named as `Closes #N` in the commits since the previous tag.
# Without a previous tag the comparisons that need one are skipped.
set -u
VERSION="${1#v}"
[ -n "$VERSION" ] || { echo "usage: check-release.sh <version>" >&2; exit 2; }
HERE="$(cd "$(dirname "$0")" && pwd)"
FAIL=0
fail() { echo "error: $*" >&2; FAIL=1; }

SEMVER='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
is_semver() { printf '%s\n' "$1" | grep -Eq "$SEMVER"; }

# prints 1 when $1 > $2 (semver precedence: numeric core, a release beats its prereleases)
greater() {
  a="${1%%+*}"; b="${2%%+*}"
  ac="${a%%-*}"; bc="${b%%-*}"
  if [ "$ac" != "$bc" ]; then
    if [ "$(printf '%s\n%s\n' "$ac" "$bc" | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)" = "$ac" ]; then echo 1; else echo 0; fi
    return
  fi
  ap=""; bp=""
  [ "$a" != "$ac" ] && ap="${a#*-}"
  [ "$b" != "$bc" ] && bp="${b#*-}"
  if [ "$ap" = "$bp" ]; then echo 0
  elif [ -z "$ap" ]; then echo 1
  elif [ -z "$bp" ]; then echo 0
  elif [ "$(printf '%s\n%s\n' "$ap" "$bp" | sort -V | tail -n 1)" = "$ap" ]; then echo 1
  else echo 0; fi
}

is_semver "$VERSION" || fail "version \"$VERSION\" is not valid semver (x.y.z)"

CARGO=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml 2>/dev/null | head -1)
[ "$CARGO" = "$VERSION" ] || fail "Cargo.toml has version \"$CARGO\", not $VERSION"
LOCK=$(awk '/^name = "shale"$/ { on = 1; next } on && /^version = / { gsub(/^version = "|"$/, ""); print; exit }' Cargo.lock 2>/dev/null)
[ "$LOCK" = "$VERSION" ] || fail "Cargo.lock has version \"$LOCK\" for shale, not $VERSION; run cargo build and commit"

# the previous release: the highest semver v* tag that is not on HEAD
HEAD_TAGS=$(git tag --points-at HEAD)
PREV=""
for t in $(git tag --list 'v*'); do
  is_semver "${t#v}" || continue
  case "
$HEAD_TAGS
" in *"
$t
"*) continue ;; esac
  if [ -z "$PREV" ] || [ "$(greater "${t#v}" "${PREV#v}")" = 1 ]; then PREV="$t"; fi
done

if [ -n "$PREV" ] && is_semver "$VERSION"; then
  [ "$(greater "$VERSION" "${PREV#v}")" = 1 ] || fail "version $VERSION is not greater than the previous release $PREV"
fi

if ! NOTES=$(sh "$HERE/release-notes.sh" "$VERSION" ./CHANGELOG.md); then
  FAIL=1
elif [ -n "$PREV" ]; then
  ISSUES=$(git log "$PREV..HEAD" --format=%B | grep -oiE 'closes +(raba/shale)?#[0-9]+' | grep -oE '[0-9]+$' | sort -un)
  MISSING=""
  for n in $ISSUES; do
    printf '%s\n' "$NOTES" | grep -Eq "#$n([^0-9]|\$)" || MISSING="$MISSING #$n"
  done
  [ -z "$MISSING" ] || fail "the CHANGELOG.md section of $VERSION does not mention the issues closed since $PREV:$MISSING"
fi

[ "$FAIL" = 0 ] && echo "Release check passed for $VERSION${PREV:+ (previous release $PREV)}."
exit "$FAIL"

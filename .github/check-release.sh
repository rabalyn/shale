#!/bin/sh
# Checks that a version may be released (spec BUILD-013). Run in the repository to check (the
# current directory), before tagging (scripts/make-tag.sh, scripts/make-release.sh) and for a
# `v*` tag in CI (Forgejo and GitHub; the checkout needs the tags and the history since the
# previous tag).
#   sh .github/check-release.sh 0.4.0
# Fails with a message per problem unless:
#   - the version is valid semver and greater than every other `v*` tag (the highest semver
#     `v*` tag that is not on HEAD, so a tag being built is not its own predecessor);
#   - Cargo.toml and the `shale` package in Cargo.lock have this version;
#   - for a release (no pre-release part such as `-rc.1`, FORGE-014): the CHANGELOG.md section of
#     the version is not empty, and mentions (as `#N` or `raba/shale#N`) every issue named as
#     `Closes #N` in the commits since the previous release (the highest `v*` tag without a
#     pre-release part that is not on HEAD). A pre-release (a check tag, FORGE-013) needs no
#     section: its notes are not published.
#   - SHALE_PREV_PUBLISHED=<tag>, when set, names the last release with a published GitHub release
#     page (the workflows and scripts look it up, BUILD-013). The tags of this repository can be
#     ahead of it (Forgejo has releases that never reached GitHub), so it replaces the previous
#     release above, and the notes checked are the ones GitHub publishes: every CHANGELOG.md
#     section after that release up to the version (.github/release-notes.sh reads the same
#     variable). It must be an existing tag, else the check fails. Unset (or empty): as above.
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

# the previous tag: the highest semver v* tag that is not on HEAD; the previous release: the
# same among the tags without a pre-release part
HEAD_TAGS=$(git tag --points-at HEAD)
PREV=""
PREV_RELEASE=""
for t in $(git tag --list 'v*'); do
  is_semver "${t#v}" || continue
  case "
$HEAD_TAGS
" in *"
$t
"*) continue ;; esac
  if [ -z "$PREV" ] || [ "$(greater "${t#v}" "${PREV#v}")" = 1 ]; then PREV="$t"; fi
  case "${t%%+*}" in *-*) continue ;; esac
  if [ -z "$PREV_RELEASE" ] || [ "$(greater "${t#v}" "${PREV_RELEASE#v}")" = 1 ]; then PREV_RELEASE="$t"; fi
done

if [ -n "$PREV" ] && is_semver "$VERSION"; then
  [ "$(greater "$VERSION" "${PREV#v}")" = 1 ] || fail "version $VERSION is not greater than the previous tag $PREV"
fi

# the last published release (SHALE_PREV_PUBLISHED) replaces the previous release for the notes
PUBLISHED="${SHALE_PREV_PUBLISHED:-}"
if [ -n "$PUBLISHED" ]; then
  if git rev-parse -q --verify "refs/tags/$PUBLISHED" >/dev/null; then
    PREV_RELEASE="$PUBLISHED"
  else
    fail "SHALE_PREV_PUBLISHED=$PUBLISHED is not a tag of this repository (fetch the tags)"
  fi
fi

case "${VERSION%%+*}" in
  *-*) PRE=yes ;;
  *) PRE=no ;;
esac

if [ "$PRE" = yes ]; then
  : # a pre-release (check tag) publishes no notes
elif ! NOTES=$(SHALE_PREV_PUBLISHED="$PUBLISHED" sh "$HERE/release-notes.sh" "$VERSION" ./CHANGELOG.md); then
  FAIL=1
elif [ -n "$PREV_RELEASE" ]; then
  ISSUES=$(git log "$PREV_RELEASE..HEAD" --format=%B | grep -oiE 'closes +(raba/shale)?#[0-9]+' | grep -oE '[0-9]+$' | sort -un)
  MISSING=""
  for n in $ISSUES; do
    printf '%s\n' "$NOTES" | grep -Eq "#$n([^0-9]|\$)" || MISSING="$MISSING #$n"
  done
  [ -z "$MISSING" ] || fail "the CHANGELOG.md section of $VERSION does not mention the issues closed since $PREV_RELEASE:$MISSING"
fi

[ "$FAIL" = 0 ] && echo "Release check passed for $VERSION${PREV:+ (previous tag $PREV)}."
exit "$FAIL"

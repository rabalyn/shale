#!/bin/sh
# Tests .github/check-release.sh on throwaway git repositories (spec BUILD-013).
#   sh .github/test-check-release.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/check-release.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0

# new_repo <name> <cargo version> <lock version>: a repo with one commit, no tags
new_repo() {
  R="$TMP/$1"
  mkdir "$R"
  cd "$R" || exit 1
  git init -q .
  git config user.email t@example.com
  git config user.name test
  git config commit.gpgsign false
  git config tag.gpgsign false
  printf '[package]\nname = "shale"\nversion = "%s"\n' "$2" > Cargo.toml
  printf '[[package]]\nname = "other"\nversion = "9.9.9"\n\n[[package]]\nname = "shale"\nversion = "%s"\n' "$3" > Cargo.lock
  printf '# Changelog\n\n## [Unreleased]\n' > CHANGELOG.md
  git add -A
  git commit -q -m "start"
}

# changelog <version> <text>: adds a section for the version
changelog() {
  printf '# Changelog\n\n## [Unreleased]\n\n## [%s] - 2026-01-01\n\n%s\n' "$1" "$2" > CHANGELOG.md
}

# expect <name> <pass|fail> <text the error output must contain> <version>
expect() {
  N=$((N + 1))
  OUT=$(sh "$CHECK" "$4" 2>&1)
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

# first release: no previous tag, the comparisons that need one are skipped
new_repo first 0.1.0 0.1.0
changelog 0.1.0 "- first"
git commit -q -am "notes"
expect "first release passes" pass "" 0.1.0
expect "a v prefix is accepted" pass "" v0.1.0

# a normal release after v0.1.0, with a closed issue mentioned in two spellings
new_repo ok 0.1.0 0.1.0
git tag -a v0.1.0 -m x
git tag latest
git commit -q --allow-empty -m "Feature" -m "Closes #7"
git commit -q --allow-empty -m "Other" -m "Closes raba/shale#8"
sed -i 's/0.1.0/0.2.0/' Cargo.toml Cargo.lock
changelog 0.2.0 "- a thing (#7), another (raba/shale#8)"
git commit -q -am "bump"
expect "greater version with notes passes" pass "" 0.2.0

# the tag of the version itself is not its own predecessor (CI runs on the tag)
git tag -a v0.2.0 -m x
expect "the tag being built passes" pass "" 0.2.0

# lower and equal versions
new_repo lower 0.1.0 0.1.0
git tag -a v0.3.0 -m x
git tag -a v0.2.0 -m x
changelog 0.1.0 "- x"
git commit -q -am "notes"
expect "a lower version fails" fail "not greater than the previous tag v0.3.0" 0.1.0
new_repo equal 0.3.0 0.3.0
git tag -a v0.3.0 -m x
git tag -a v0.10.0-rc.1 -m x
git commit -q --allow-empty -m "later"
changelog 0.3.0 "- x"
git commit -q -am "notes"
expect "an equal version fails (and 0.10.0-rc.1 outranks 0.3.0)" fail "not greater than the previous tag v0.10.0-rc.1" 0.3.0
git tag -d v0.10.0-rc.1 >/dev/null
expect "an equal version fails" fail "not greater than the previous tag v0.3.0" 0.3.0
expect "not semver fails" fail "not valid semver" 1.0

# Cargo.lock and Cargo.toml mismatches
new_repo lock 0.2.0 0.1.0
changelog 0.2.0 "- x"
git commit -q -am "notes"
expect "a Cargo.lock mismatch fails" fail "Cargo.lock has version \"0.1.0\" for shale" 0.2.0
expect "a Cargo.toml mismatch fails" fail "Cargo.toml has version \"0.2.0\", not 0.3.0" 0.3.0

# changelog sections
new_repo empty 0.2.0 0.2.0
git tag -a v0.1.0 -m x
expect "no changelog section fails" fail "no notes for version 0.2.0" 0.2.0
changelog 0.2.0 ""
git commit -q -am "notes"
expect "an empty changelog section fails" fail "no notes for version 0.2.0" 0.2.0

# issues closed since the previous tag must be in the section
new_repo missing 0.2.0 0.2.0
git tag -a v0.1.0 -m x
git commit -q --allow-empty -m "A" -m "Closes #12"
git commit -q --allow-empty -m "B" -m "Closes #3"
git commit -q --allow-empty -m "C" -m "Refs #99"
changelog 0.2.0 "- only #123 and #30 here"
git commit -q -am "notes"
expect "missing issues fail and are listed" fail "since v0.1.0: #3 #12" 0.2.0

# an issue closed before the previous tag does not count
new_repo old 0.2.0 0.2.0
git commit -q --allow-empty -m "A" -m "Closes #5"
git tag -a v0.1.0 -m x
changelog 0.2.0 "- nothing about it"
git commit -q -am "notes"
expect "issues closed before the previous tag are ignored" pass "" 0.2.0

# pre-releases (check tags, FORGE-013): no changelog section needed, still greater and matching
new_repo pre 0.4.0-rc.1 0.4.0-rc.1
git tag -a v0.3.0 -m x
git commit -q --allow-empty -m "A" -m "Closes #40"
expect "a pre-release needs no changelog section" pass "" 0.4.0-rc.1
expect "a pre-release must match Cargo.toml" fail "Cargo.toml has version \"0.4.0-rc.1\", not 0.4.0-rc.2" 0.4.0-rc.2
git tag -a v0.4.0-rc.1 -m x
git commit -q --allow-empty -m "later"
expect "a pre-release is not greater than the previous one" fail "not greater than the previous tag v0.4.0-rc.1" 0.4.0-rc.1
sed -i 's/0.4.0-rc.1/0.4.0-rc.2/' Cargo.toml Cargo.lock
git commit -q -am "rc.2"
expect "the next pre-release passes" pass "" 0.4.0-rc.2
git tag -a v0.4.0-rc.2 -m x
git commit -q --allow-empty -m "B" -m "Closes #41"

# a release after pre-releases: its section covers everything since the previous release
sed -i 's/0.4.0-rc.2/0.4.0/' Cargo.toml Cargo.lock
changelog 0.4.0 "- only #41"
git commit -q -am "release"
expect "a release counts issues since the previous release, not the pre-release" fail "since v0.3.0: #40" 0.4.0
changelog 0.4.0 "- #40 and #41"
git commit -q -am "notes"
expect "a release after its pre-releases passes" pass "" 0.4.0

[ "$FAILED" = 0 ] && echo "all $N checks passed"
exit "$FAILED"

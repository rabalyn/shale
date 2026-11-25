#!/bin/bash
# Tests scripts/make-tag.sh and scripts/make-release.sh (FORGE-013, FORGE-014, FORGE-020, ADR-0018) on a
# throwaway remote and clone. The cargo checks of land.sh are replaced by LAND_CHECK_CMD, the
# GitHub CLI by a stub that records its calls and the benchmark program (BUILD-034) by a stub
# that writes small result files, so the test is about versions, the changelog, landing, tagging,
# the stored performance results and refusing.
#   bash scripts/test-release.sh
set -u
# the performance tests filter real JSON with jq (scripts/bench-release.sh): without it they would pass or fail
# for the wrong reason (#522), so stop here, plainly
if ! command -v jq >/dev/null 2>&1; then
  echo "not ok 1 - jq is missing: install jq (apt-get install jq), the performance tests of the release scripts need it"
  exit 1
fi
# a release that runs land.sh and its cargo test must not leak its value in here (#406); the cases set it
unset SHALE_PREV_PUBLISHED
SRC="$(cd "$(dirname "$0")/.." && pwd)"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
export GIT_CONFIG_GLOBAL="$TMP/gitconfig"
git config --global user.email t@example.com
git config --global user.name test
git config --global commit.gpgsign false
git config --global tag.gpgsign false
git config --global init.defaultBranch main

# the GitHub CLI: `issue list` answers the issue made by `issue create` (77) once there is one
mkdir "$TMP/bin"
cat > "$TMP/bin/gh" <<'GH'
#!/bin/sh
echo "$*" >> "$GH_LOG"
# a comment from a file (the performance numbers of a release): keep what it said
prev=""
for a in "$@"; do
  [ "$prev" = --body-file ] && cat "$a" >> "$GH_LOG.body"
  prev=$a
done
case "$1 $2" in
  "issue list")
    # a test hook, run once: what happens while the release runs (#348)
    if [ -n "${GH_HOOK:-}" ] && [ -x "$GH_HOOK" ]; then "$GH_HOOK" < /dev/null; rm -f "$GH_HOOK"; fi
    [ -f "$GH_LOG.made" ] && echo 77
    exit 0 ;;
  "release view")
    # the last published release (BUILD-013): GH_RELEASE_FAIL makes gh fail, GH_RELEASE_TAG names the tag
    [ -z "${GH_RELEASE_FAIL:-}" ] || { echo "gh: no network" >&2; exit 1; }
    echo "${GH_RELEASE_TAG:-}" ;;
  "run list")
    # the main-check runs on GitHub (BUILD-039): GH_RUN_FAIL makes gh fail, GH_RUN_OUT is the list (tab-separated)
    [ -z "${GH_RUN_FAIL:-}" ] || { echo "gh: no network" >&2; exit 1; }
    [ -z "${GH_RUN_OUT:-}" ] || printf '%b\n' "$GH_RUN_OUT" ;;
  "issue create") touch "$GH_LOG.made"; echo "https://github.com/rabalyn/shale/issues/77" ;;
esac
exit 0
GH
chmod +x "$TMP/bin/gh"
# the benchmark program: `--bench` writes a result, `--bench-compare` exits with FAKE_COMPARE_EXIT
cat > "$TMP/bin/fake-shale" <<'FAKE'
#!/bin/sh
echo "$*" >> "$BENCH_LOG"
case "$1" in
  --bench)
    mkdir -p "$(dirname "$2")"
    printf '{"schema":1,"commit":"%s","describe":"%s","date":"2026-01-01T00:00:00Z","host":{"cpu":"c","cores":1,"os":"o"},"profile":"bench-release","scenarios":[],"peak_rss_kb":null,"run":"%s"}\n' "$SHALE_BENCH_COMMIT" "$SHALE_BENCH_DESCRIBE" "$4" > "$2" ;;
  --bench-merge) mkdir -p "$(dirname "$2")"; cp "$3" "$2" ;;
  --bench-compare)
    while [ $# -gt 0 ]; do [ "$1" = --out ] && dir=$2; shift; done
    mkdir -p "$dir"
    echo "# compare" > "$dir/compare.md"
    # FAKE_COMPARE_JSON: a file with the compare.json to write (the regressions it names)
    if [ -n "${FAKE_COMPARE_JSON:-}" ]; then cp "$FAKE_COMPARE_JSON" "$dir/compare.json"; else echo '{"regressions":[]}' > "$dir/compare.json"; fi
    exit "${FAKE_COMPARE_EXIT:-0}" ;;
  --bench-report) echo "<html></html>" > "$3" ;;
esac
FAKE
chmod +x "$TMP/bin/fake-shale"
# scripts/bench-compare.sh, which a release uses to measure flagged scenarios again (BUILD-034): it records
# its arguments and writes FAKE_RECHECK_JSON as the compare.json (no regression without it)
cat > "$TMP/bin/fake-bench-compare" <<'FAKE'
#!/bin/sh
echo "bench-compare $*" >> "$BENCH_LOG"
while [ $# -gt 0 ]; do [ "$1" = --out ] && dir=$2; shift; done
# like the real one, which changes into the folder of its own checkout: a relative --out would be lost
cd "$(dirname "$BENCH_LOG")" || exit 2
mkdir -p "$dir"
rm -f "$dir/compare.json"
echo "# recheck" > "$dir/compare.md"
cp "$FAKE_RECHECK_JSON" "$dir/compare.json"
exit "${FAKE_RECHECK_EXIT:-0}"
FAKE
chmod +x "$TMP/bin/fake-bench-compare"
export BENCH_BIN="$TMP/bin/fake-shale" BENCH_COMPARE="$TMP/bin/fake-bench-compare" BENCH_LOG="$TMP/bench.log"
export RELEASE_GH="$TMP/bin/gh" GH_LOG="$TMP/gh.log" RELEASE_DATE=2026-11-17 LAND_CHECK_CMD=true LAND_REPORT_CMD=true
# the size report (BUILD-045) builds the whole program: a stub that writes what it was given, and notes that it ran
export SHALE_SIZE_REPORT_CMD='printf "size report of $SHALE_VERSION on $SHALE_DATE\n" > docs/size-report.md; echo run >> "$SIZE_LOG"' SIZE_LOG="$TMP/size.log"

git init -q --bare "$TMP/remote.git"
git clone -q "$TMP/remote.git" "$TMP/work" 2>/dev/null
cd "$TMP/work" || exit 1
mkdir -p scripts .github docs/changelog-pending
cp "$SRC"/scripts/{land.sh,release-lib.sh,make-tag.sh,make-release.sh,bench-release.sh,bench-env.sh} scripts/
cp "$SRC"/.github/{check-release.sh,release-notes.sh,ai-notice.md} .github/
printf '[package]\nname = "shale"\nversion = "0.3.0"\nedition = "2021"\n\n[dependencies]\nother = { version = "9.9.9" }\n' > Cargo.toml
printf '[[package]]\nname = "other"\nversion = "9.9.9"\n\n[[package]]\nname = "shale"\nversion = "0.3.0"\n' > Cargo.lock
cat > CHANGELOG.md <<'CL'
# Changelog

Intro text.

## [Unreleased]

### Added

- Unreleased thing (#4).

## [0.3.0] - 2026-11-13

### Fixed

- Old fix.

[Unreleased]: https://github.com/rabalyn/shale/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/rabalyn/shale/compare/v0.2.0...v0.3.0
CL
git add -A && git commit -q -m "Start (Refs #1)" && git tag -a v0.3.0 -m x && git push -q origin main v0.3.0

# land <name> <file> <text> [message]: a change landed on main like any other
land() {
  git checkout -q -b "$1" origin/main
  mkdir -p "$(dirname "$2")"
  printf '%s\n' "$3" > "$2"
  git add -A && git commit -q -m "${4:-$1 (Refs #1)}"
  git checkout -q main
  bash scripts/land.sh "$1" >/dev/null 2>&1 || echo "setup: landing $1 failed"
}

# expect <name> <0|1> <needle> <script> <args>…: runs a script, checks the exit code and the output
expect() {
  local name=$1 code=$2 needle=$3 script=$4
  shift 4
  N=$((N + 1))
  OUT=$(bash "scripts/$script" "$@" 2>&1)
  GOT=$?
  if [ "$GOT" = "$code" ] && printf '%s' "$OUT" | grep -Fq -- "$needle"; then
    echo "ok $N - $name"
  else
    echo "not ok $N - $name (exit $GOT): $OUT"
    FAILED=1
  fi
}
check() {
  N=$((N + 1))
  if eval "$2"; then echo "ok $N - $1"; else echo "not ok $N - $1"; FAILED=1; fi
}
remote_tag() { git ls-remote --tags origin "refs/tags/$1" | grep -q .; }
version_on_main() { git fetch -q origin; git show origin/main:Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -1; }

land pend1 docs/changelog-pending/I10.md $'### Changed\n\n- Ten (#10).' "Ten (Closes #10)"
land pend2 docs/changelog-pending/I2.md $'### Added\n\n- Two (#2).\n\n### Tests\n\n- A test.' "Two (Closes #2)"

# check tags (FORGE-013)
expect "the first check tag needs a level" 1 "say patch, minor or major" make-tag.sh
expect "a dry run names the tag" 0 "check tag: v0.4.0-rc.1" make-tag.sh --dry-run minor
check "build_045: a dry run makes no size report" '[ ! -e "$SIZE_LOG" ]'
check "a dry run changes nothing" '! remote_tag v0.4.0-rc.1 && [ "$(version_on_main)" = 0.3.0 ] && [ ! -s "$GH_LOG" ]'
expect "a check tag is not a release" 1 "a check tag is a pre-release" make-tag.sh 0.4.0
expect "minor makes the first rc of the next minor version" 0 "pushed the tag to origin" make-tag.sh minor
check "the tag is on the remote and points at the landed bump" 'remote_tag v0.4.0-rc.1 && [ "$(git rev-parse "v0.4.0-rc.1^{commit}")" = "$(git rev-parse origin/main)" ]'
check "main has the version in Cargo.toml and Cargo.lock" '[ "$(version_on_main)" = 0.4.0-rc.1 ] && git show origin/main:Cargo.lock | grep -A1 "name = \"shale\"" | grep -Fq "0.4.0-rc.1" && git show origin/main:Cargo.lock | grep -Fq "version = \"9.9.9\"" && git show origin/main:Cargo.toml | grep -Fq "other = { version = \"9.9.9\" }"'
check "the tag is annotated" '[ "$(git cat-file -t v0.4.0-rc.1)" = tag ]'
check "build_045: the size report is in the version commit of a check tag" '[ "$(git show v0.4.0-rc.1:docs/size-report.md)" = "size report of 0.4.0-rc.1 on 2026-11-17" ] && git show --stat --format= "origin/main^2" | grep -Fq docs/size-report.md'
check "the release issue was made and named in the commit" 'grep -Fq "issue create -R rabalyn/shale --title Release 0.4.0" "$GH_LOG" && git log -1 --format=%B "origin/main^2" | grep -Fq "Version 0.4.0-rc.1 (check tag, Refs #77)"'
check "a check tag merges the pending files into [Unreleased], without a version section" '[ -z "$(git ls-tree origin/main docs/changelog-pending/)" ] && git show origin/main:CHANGELOG.md | sed -n "/^## \[Unreleased\]/,/^## \[0.3.0\]/p" | grep -Fq -- "- Two (#2)." && ! git show origin/main:CHANGELOG.md | grep -Fq "0.4.0"'
check "the local main followed and no release branch is left" '[ "$(git rev-parse main)" = "$(git rev-parse origin/main)" ] && ! git rev-parse -q --verify refs/heads/release/v0.4.0-rc.1 >/dev/null'
expect "a check tag without a changelog update is refused" 1 "the changelog needs an update first" make-tag.sh --dry-run
land pend3 docs/changelog-pending/I3.md $'### Fixed\n\n- Three (#3).'
expect "without a level the next rc follows" 0 "check tag: v0.4.0-rc.2" make-tag.sh
check "the open release issue is reused" '[ "$(grep -c "issue create" "$GH_LOG")" = 1 ] && grep -Fq "issue comment 77" "$GH_LOG"'
land pend4 docs/changelog-pending/I4.md $'### Fixed\n\n- Four (#4).'
expect "minor again continues the rc of that version" 0 "check tag: v0.4.0-rc.3" make-tag.sh --dry-run minor
expect "an existing tag is refused" 1 "exists already" make-tag.sh 0.4.0-rc.2
printf 'dirty\n' >> Cargo.toml
expect "uncommitted changes are refused" 1 "uncommitted changes" make-tag.sh
git checkout -q -- Cargo.toml
LAND_CHECK_CMD=false expect "a failing landing tags nothing" 1 "nothing was tagged" make-tag.sh --issue 5
check "after a failing landing no tag or branch is left" '! remote_tag v0.4.0-rc.3 && ! git rev-parse -q --verify refs/heads/release/v0.4.0-rc.3 >/dev/null && [ "$(version_on_main)" = 0.4.0-rc.2 ]'

# releases (FORGE-014)
land closes9 src.txt nine "Nine (Closes #9)"
GH_RELEASE_FAIL=1 expect "a gh that cannot tell the last published release stops the release (BUILD-013)" 1 "could not tell the last published release" make-release.sh
check "the release check asked for the last published release of the repository" 'grep -Fq "release view -R rabalyn/shale --json tagName -q .tagName" "$GH_LOG"'
expect "an issue missing from the notes stops the release before landing" 1 "#9" make-release.sh
check "nothing landed or tagged after the failed check" '! remote_tag v0.4.0 && [ "$(version_on_main)" = 0.4.0-rc.2 ] && ! git rev-parse -q --verify refs/heads/release/v0.4.0 >/dev/null'
land notes9 docs/changelog-pending/I9.md $'### Fixed\n\n- Nine (#9).'
expect "a release is not a pre-release" 1 "has no pre-release part" make-release.sh 0.4.0-rc.9
# the main-check of GitHub (BUILD-039, #453); the latest run that was not cancelled decides
R_GREEN='completed\tsuccess\taaaaaaa1111\thttps://example/run/1'
R_RED='completed\tfailure\tbbbbbbb2222\thttps://example/run/2'
R_RUN='in_progress\t\tccccccc3333\thttps://example/run/3'
R_CANCEL='completed\tcancelled\tddddddd4444\thttps://example/run/4'
GH_RUN_OUT="$R_RED" expect "a red main-check on GitHub stops the release" 1 "is failure" make-release.sh 0.4.0
GH_RUN_OUT="$R_RUN\n$R_GREEN" expect "a main-check that is still running stops the release" 1 "is still in_progress" make-release.sh 0.4.0
GH_RUN_OUT="$R_CANCEL\n$R_RED" expect "a cancelled run does not count: the red one before it stops" 1 "is failure" make-release.sh 0.4.0
GH_RUN_FAIL=1 expect "a gh that cannot list the runs stops the release" 1 "could not list the main-check runs" make-release.sh 0.4.0
GH_RUN_OUT="$R_RED" expect "--ignore-github-check skips the check" 0 "not looked at" make-release.sh --dry-run --ignore-github-check 0.4.0
GH_RUN_OUT="$R_RED" expect "a dry run reports a red main-check and goes on" 0 "a release would stop here" make-release.sh --dry-run 0.4.0
GH_RUN_OUT="$R_CANCEL\n$R_GREEN\n$R_RED" expect "a green main-check lets the release go on" 0 "main-check is green for aaaaaaa" make-release.sh --dry-run 0.4.0
check "the check asked GitHub for the runs of main-check.yml on main" 'grep -Fq "run list -R rabalyn/shale --workflow main-check.yml --branch main" "$GH_LOG"'
SHALE_SIZE_REPORT_CMD=false expect "build_045: a failing size report stops a release before anything is landed" 1 "the size report failed" make-release.sh
check "build_045: nothing was landed or tagged after the failing size report" '! remote_tag v0.4.0 && [ "$(version_on_main)" = 0.4.0-rc.2 ] && ! git rev-parse -q --verify refs/heads/release/v0.4.0 >/dev/null'
SHALE_SIZE_REPORT_CMD=false expect "build_045: a failing size report stops a check tag" 1 "the size report failed" make-tag.sh --issue 5 0.4.0-rc.3
check "build_045: nothing was landed or tagged after the failing size report of a check tag" '! remote_tag v0.4.0-rc.3 && [ "$(version_on_main)" = 0.4.0-rc.2 ]'
expect "the release of the rc version" 0 "release tag: v0.4.0" make-release.sh
check "build_045: the size report is in the release commit" '[ "$(git show v0.4.0:docs/size-report.md)" = "size report of 0.4.0 on 2026-11-17" ]'
check "main and the tag have version 0.4.0" 'remote_tag v0.4.0 && [ "$(version_on_main)" = 0.4.0 ] && [ "$(git rev-parse "v0.4.0^{commit}")" = "$(git rev-parse origin/main)" ]'
git show origin/main:CHANGELOG.md > "$TMP/cl"
cat > "$TMP/want" <<'CL'
# Changelog

Intro text.

## [Unreleased]

## [0.4.0] - 2026-11-17

### Added

- Unreleased thing (#4).
- Two (#2).

### Changed

- Ten (#10).

### Fixed

- Three (#3).
- Four (#4).
- Nine (#9).

### Tests

- A test.

## [0.3.0] - 2026-11-13

### Fixed

- Old fix.

[Unreleased]: https://github.com/rabalyn/shale/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/rabalyn/shale/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rabalyn/shale/compare/v0.2.0...v0.3.0
CL
check "the changelog section merges [Unreleased] and the pending files by heading" 'diff -u "$TMP/want" "$TMP/cl"'
check "the pending files are gone" '[ -z "$(git ls-tree origin/main docs/changelog-pending/)" ]'
check "the release commit names the issue" 'git log -1 --format=%s "origin/main^2" | grep -Fxq "Release 0.4.0 (Refs #77)"'
check "the tag message names the version" '[ "$(git tag -l --format="%(contents:subject)" v0.4.0)" = "Shale 0.4.0" ]'
# the performance step of a release (BUILD-034)
check "the release stored the full and the worst result and the report" 'git show origin/main:docs/perf/results/v0.4.0.json | grep -Fq "\"describe\":\"v0.4.0\"" && git show origin/main:docs/perf/results-worst/v0.4.0.json | grep -Fq "\"run\":\"worst\"" && git show origin/main:docs/perf/index.html | grep -Fq html'
check "the full profile ran with its name and the worst one too" 'grep -Fq -- "--profile full" "$BENCH_LOG" && grep -Fq -- "--profile worst" "$BENCH_LOG"'
check "without an earlier release nothing is compared" '! grep -Fq -- "--bench-compare" "$BENCH_LOG" && [ -z "$(git ls-tree origin/main docs/perf/compare/)" ]'
expect "a release needs changelog entries" 1 "the changelog needs an update first" make-release.sh patch
land stray docs/changelog-pending/I11.md "- not under a heading"
expect "a changelog line outside a heading is refused and named" 1 "outside a ### heading in docs/changelog-pending/I11.md" make-release.sh patch
check "no release worktree is left behind" '! git worktree list | grep -Fq release/worktree'

# a landing during a release (FORGE-014, #348)
git checkout -q -b unstray origin/main && git rm -q docs/changelog-pending/I11.md && git commit -q -m "unstray (Refs #1)" && git checkout -q main
bash scripts/land.sh unstray >/dev/null 2>&1 || echo "setup: landing unstray failed"
land pend20 docs/changelog-pending/I20.md $'### Fixed\n\n- Twenty (#20).' "Twenty (Closes #20)"
git checkout -q -b racer origin/main
mkdir -p docs/changelog-pending && printf '### Fixed\n\n- Twelve (#12).\n' > docs/changelog-pending/I12.md
git add -A && git commit -q -m "Twelve (Closes #12)" && git checkout -q main
# the hook starts a landing while the release holds the lock (it runs after the lock is taken)
cat > "$TMP/hook-racer" <<HOOK
#!/bin/sh
cd "$TMP/work" || exit 1
( bash scripts/land.sh racer; echo \$? > "$TMP/racer.rc" ) > "$TMP/racer.out" 2>&1 < /dev/null 8>&- &
i=0
while ! grep -Fq "waiting" "$TMP/racer.out" && [ \$i -lt 100 ]; do sleep 0.1; i=\$((i + 1)); done
HOOK
chmod +x "$TMP/hook-racer"
GH_HOOK="$TMP/hook-racer" expect "a release holds the landing lock until its tag" 0 "pushed the tag to origin" make-release.sh patch
i=0; while [ ! -f "$TMP/racer.rc" ] && [ $i -lt 300 ]; do sleep 0.1; i=$((i + 1)); done
check "the landing that came meanwhile waited and then landed" 'grep -Fq "waiting" "$TMP/racer.out" && [ "$(cat "$TMP/racer.rc")" = 0 ] && git fetch -q origin && git show origin/main:docs/changelog-pending/I12.md >/dev/null'
check "it landed after the tag, not inside it" 'remote_tag v0.4.1 && git merge-base --is-ancestor "v0.4.1^{commit}" origin/main && [ "$(git rev-parse "v0.4.1^{commit}")" != "$(git rev-parse origin/main)" ] && ! git cat-file -e "v0.4.1^{commit}:docs/changelog-pending/I12.md" 2>/dev/null'
git worktree add -q --detach "$TMP/chk" v0.4.1
check "the tagged release passes the release check" '(cd "$TMP/chk" && sh .github/check-release.sh 0.4.1 >/dev/null 2>&1)'
git worktree remove --force "$TMP/chk"

# a push by hand that bypasses the lock lands in the release; the check refuses to tag it
cat > "$TMP/hook-hand" <<HOOK
#!/bin/sh
rm -rf "$TMP/other"
git clone -q "$TMP/remote.git" "$TMP/other" 2>/dev/null
cd "$TMP/other" || exit 1
mkdir -p docs/changelog-pending
printf '### Fixed\n\n- Thirteen (#13).\n' > docs/changelog-pending/I13.md
git add -A && git commit -q -m "Thirteen (Closes #13)" && git push -q origin main
HOOK
chmod +x "$TMP/hook-hand"
GH_HOOK="$TMP/hook-hand" expect "a landed merge that fails the check is not tagged, and the message names the issue" 1 "#13" make-release.sh patch
check "the message says what to do, nothing was tagged and main has the version" 'printf "%s" "$OUT" | grep -Fq "run make-release.sh 0.4.2 again" && ! remote_tag v0.4.2 && ! git rev-parse -q --verify refs/tags/v0.4.2 >/dev/null && [ "$(version_on_main)" = 0.4.2 ]'
check "no check worktree is left behind" '! git worktree list | grep -Fq release/check'
# the recovery of the message: the entry in a branch, landed, then the same version again
git checkout -q -b fix13 origin/main
git rm -q docs/changelog-pending/I13.md
awk '{ print } /^## \[0.4.2\]/ { print ""; print "### Fixed"; print ""; print "- Thirteen (#13)." }' CHANGELOG.md > CHANGELOG.md.new && mv CHANGELOG.md.new CHANGELOG.md
git add -A && git commit -q -m "Thirteen in the notes (Refs #13)" && git checkout -q main
bash scripts/land.sh fix13 >/dev/null 2>&1 || echo "setup: landing fix13 failed"
expect "a dry run of the rerun says it tags main" 0 "on origin/main already" make-release.sh --dry-run 0.4.2
expect "the same version again tags the landed main" 0 "pushed the tag to origin" make-release.sh 0.4.2
check "the recovery tag points at main and passes" 'remote_tag v0.4.2 && [ "$(git rev-parse "v0.4.2^{commit}")" = "$(git rev-parse origin/main)" ]'

# the performance step of later releases (BUILD-034)
land perf1 docs/changelog-pending/I30.md $'### Fixed\n\n- Thirty (#30).'
FAKE_COMPARE_EXIT=1 expect "a performance regression stops the release" 1 "performance regression" make-release.sh patch
check "nothing was landed or tagged after the regression" '! remote_tag v0.4.3 && [ "$(version_on_main)" = 0.4.2 ] && ! git rev-parse -q --verify refs/heads/release/v0.4.3 >/dev/null'
FAKE_COMPARE_EXIT=1 expect "--allow-perf-regression lets a regression pass and says so" 0 "allowed by --allow-perf-regression" make-release.sh --allow-perf-regression patch
check "the release compared with the last one and stored the comparison" 'remote_tag v0.4.3 && grep -Fq -- "--bench-compare docs/perf/results/v0.4.2.json docs/perf/results/v0.4.3.json" "$BENCH_LOG" && git show origin/main:docs/perf/compare/v0.4.3.md | grep -Fq compare && git show origin/main:docs/perf/compare/v0.4.3-worst.json | grep -Fq regressions'
# spec: BUILD-037
check "the full profile ran twice and the runs were merged, the worst profile once" '[ "$(grep -c -- "^--bench .* --profile full" "$BENCH_LOG")" -ge 2 ] && grep -Fq -- "--bench-merge docs/perf/results/v0.4.3.json" "$BENCH_LOG" && [ "$(grep -c -- "^--bench .* --profile worst" "$BENCH_LOG")" -eq "$(grep -c -- "--bench-merge docs/perf/results-worst/" "$BENCH_LOG")" ]'
land perf2 docs/changelog-pending/I31.md $'### Fixed\n\n- Thirty-one (#31).'
expect "--skip-bench leaves the performance step out" 0 "not measured" make-release.sh --skip-bench patch
check "a skipped release has no result" 'remote_tag v0.4.4 && ! git cat-file -e origin/main:docs/perf/results/v0.4.4.json 2>/dev/null'
land perf3 docs/changelog-pending/I32.md $'### Fixed\n\n- Thirty-two (#32).'
FAKE_COMPARE_EXIT=2 expect "a failing comparison stops the release too" 1 "the performance step failed" make-release.sh patch
# spec: BUILD-013, FORGE-014: the last published release is for the release check only (#406): land.sh and
# its checks (cargo test runs the tests of these scripts) must not see it
LAND_CHECK_CMD="echo ran >> $TMP/land-env.log; env | grep ^SHALE_PREV_PUBLISHED= >> $TMP/land-env.log; true" GH_RELEASE_TAG=v0.3.0 expect "a release with the last published release named by gh goes on (BUILD-013)" 0 "tagged" make-release.sh patch
check "the checks of land.sh did not see SHALE_PREV_PUBLISHED (#406)" '[ "$(command grep -c ^ran "$TMP/land-env.log")" -ge 1 ] && [ "$(command grep -c ^SHALE_PREV "$TMP/land-env.log")" = 0 ]'
check "it compares with the last release that has a result" 'grep -Fq -- "--bench-compare docs/perf/results/v0.4.3.json docs/perf/results/v0.4.5.json" "$BENCH_LOG"'

# spec: FORGE-013, FORGE-014: a changelog bigger than the pipe buffer (#466): has_entries must read
# all of it, or git show dies of SIGPIPE and pipefail reports "no entries" for a filled [Unreleased]
git checkout -q -b bigcl origin/main
{
  awk '/^\[Unreleased\]: / { exit } { print }' CHANGELOG.md
  for i in $(seq 1 3000); do printf '## [0.0.%s] - 2026-01-01\n\n### Fixed\n\n- An old fix that pads the file to make it big, number %s.\n\n' "$i" "$i"; done
  awk '/^\[Unreleased\]: / { on = 1 } on { print }' CHANGELOG.md
} > CHANGELOG.md.big
mv CHANGELOG.md.big CHANGELOG.md
git add -A && git commit -q -m "Big changelog (Refs #1)"
git checkout -q main
bash scripts/land.sh bigcl >/dev/null 2>&1 || echo "setup: landing bigcl failed"
check "the changelog is bigger than 200 KB (#466)" '[ "$(git show origin/main:CHANGELOG.md | wc -c)" -gt 200000 ]'
expect "a big changelog with an empty [Unreleased] still stops a release (#466)" 1 "the changelog needs an update first" make-release.sh --dry-run patch
expect "a big changelog with an empty [Unreleased] still stops a check tag (#466)" 1 "the changelog needs an update first" make-tag.sh --dry-run patch
git checkout -q -b bigcl2 origin/main
awk '{ print } /^## \[Unreleased\]/ { printf "\n### Fixed\n\n- Big (#466).\n" }' CHANGELOG.md > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md
git add -A && git commit -q -m "Entry in a big changelog (Refs #466)"
git checkout -q main
bash scripts/land.sh bigcl2 >/dev/null 2>&1 || echo "setup: landing bigcl2 failed"
OUT=$(bash scripts/make-release.sh --dry-run patch 2>&1)
check "a big changelog with an [Unreleased] entry does not stop a release dry run (#466)" '! printf "%s" "$OUT" | grep -Fq "needs an update first" && printf "%s" "$OUT" | grep -Fq "v0.4."'

# spec: BUILD-034 (#408): a flagged time regression is measured again, interleaved, before it stops a release;
# allocations and peak bytes are exact and stand as they are
FIRST=$(git rev-parse origin/main)
cat > "$TMP/time-reg.json" <<J
{"verdict":"regression","x":{"commit":"$FIRST"},"regressions":[
 {"id":"ui.frame.x","metric":"time","x":1000000,"y":2000000,"delta_pct":100.0,"confidence":"low"},
 {"id":"vault.open.large","metric":"time","x":5000000,"y":6000000,"delta_pct":20.0,"confidence":"low"}]}
J
cat > "$TMP/alloc-reg.json" <<J
{"verdict":"regression","x":{"commit":"$FIRST"},"regressions":[{"id":"ui.frame.x","metric":"allocs","x":1000,"y":2000,"delta_pct":100.0,"confidence":"high"}]}
J
cat > "$TMP/recheck-noise.json" <<'J'
{"verdict":"ok","x":{"describe":"v0.4.5","commit":"aaaaaaaaaaaa","load":1.5},"y":{"repeats":5,"load":1.8},"regressions":[],
 "rows":[{"id":"ui.frame.x","x_median_ns":1000000,"y_median_ns":1010000,"time_delta_pct":1.0,"time_verdict":"ok"},
         {"id":"vault.open.large","x_median_ns":5000000,"y_median_ns":5100000,"time_delta_pct":2.0,"time_verdict":"ok"}]}
J
cat > "$TMP/recheck-real.json" <<'J'
{"verdict":"regression","x":{"describe":"v0.4.5","commit":"aaaaaaaaaaaa","load":1.5},"y":{"repeats":5,"load":1.8},
 "regressions":[{"id":"ui.frame.x","metric":"time","x":1000000,"y":1900000,"delta_pct":90.0,"confidence":"high"}],
 "rows":[{"id":"ui.frame.x","x_median_ns":1000000,"y_median_ns":1900000,"time_delta_pct":90.0,"time_verdict":"REGRESSION"},
         {"id":"vault.open.large","x_median_ns":5000000,"y_median_ns":5100000,"time_delta_pct":2.0,"time_verdict":"ok"}]}
J
land perf4 docs/changelog-pending/I33.md $'### Fixed\n\n- Thirty-three (#33).'
: > "$BENCH_LOG"; rm -f "$GH_LOG.body"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" FAKE_RECHECK_EXIT=1 expect "a time regression that shows again in the re-run stops the release" 1 "performance regression" make-release.sh patch
check "only the confirmed regression is listed, with the numbers of the re-run" 'printf "%s" "$OUT" | grep -Fq "ui.frame.x time 1000000 -> 1900000" && ! printf "%s" "$OUT" | grep -Fq "vault.open.large time" && ! printf "%s" "$OUT" | grep -Fq "2000000"'
check "the flagged scenarios alone were measured again against the previous release, interleaved, 5 runs" 'grep -Fq "bench-compare $FIRST " "$BENCH_LOG" && grep -Fq -- "--profile full --runs 5 --filter =ui.frame.x,=vault.open.large --out" "$BENCH_LOG" && grep -Fq -- "--profile worst --runs 5 --filter =ui.frame.x,=vault.open.large" "$BENCH_LOG"'
check "the numbers went into the release issue although the release stopped" 'grep -Fq "issue comment 77 -R rabalyn/shale --body-file" "$GH_LOG" && grep -Fq "| \`ui.frame.x\` | +100 % | 1 | 1.9 | 90 % | REGRESSION |" "$GH_LOG.body" && grep -Fq "**Confirmed**: ui.frame.x" "$GH_LOG.body" && grep -Fq "| \`vault.open.large\` | +20 % | 5 | 5.1 | 2 % | ok |" "$GH_LOG.body"'
check "nothing was landed or tagged" '! remote_tag v0.4.6 && ! git rev-parse -q --verify refs/heads/release/v0.4.6 >/dev/null'
: > "$BENCH_LOG"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" FAKE_RECHECK_EXIT=1 expect "--allow-perf-regression lets a confirmed time regression pass" 0 "allowed by --allow-perf-regression" make-release.sh --allow-perf-regression patch
check "the re-run is kept in docs/perf/compare, and the first comparison stays" 'remote_tag v0.4.6 && git show origin/main:docs/perf/compare/v0.4.6-recheck.json | grep -Fq REGRESSION && git show origin/main:docs/perf/compare/v0.4.6-recheck.md | grep -Fq recheck && git show origin/main:docs/perf/compare/v0.4.6-worst-recheck.json | grep -Fq REGRESSION && git show origin/main:docs/perf/compare/v0.4.6.json | grep -Fq "\"delta_pct\":100.0"'
land perf5 docs/changelog-pending/I34.md $'### Fixed\n\n- Thirty-four (#34).'
: > "$BENCH_LOG"; rm -f "$GH_LOG.body"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-noise.json" BENCH_MAX_LOAD=6 expect "a time regression that does not show again is noise and the release goes on" 0 "not confirmed by the re-run" make-release.sh patch
check "the release is made, the re-run was kept and said so in the issue; --max-load was passed on" 'remote_tag v0.4.7 && git show origin/main:docs/perf/compare/v0.4.7-recheck.json | grep -Fq "\"verdict\":\"ok\"" && grep -Fq "**Not confirmed**" "$GH_LOG.body" && grep -Fq -- "--max-load 6" "$BENCH_LOG"'
land perf6 docs/changelog-pending/I35.md $'### Fixed\n\n- Thirty-five (#35).'
: > "$BENCH_LOG"
FAKE_COMPARE_JSON="$TMP/alloc-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-noise.json" expect "an allocation regression is exact: it stops the release without a re-run" 1 "ui.frame.x allocs 1000 -> 2000" make-release.sh patch
check "nothing was measured again" '! grep -q "^bench-compare " "$BENCH_LOG"'
: > "$BENCH_LOG"
echo '{"schema":2,"verdict":"error","exit_code":2,"error":"the build failed"}' > "$TMP/recheck-error.json"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-error.json" FAKE_RECHECK_EXIT=2 expect "a re-run that fails stops the release as a failed step" 1 "the performance step failed" make-release.sh patch

# spec: BUILD-034 (#520): bench-release.sh itself. A re-run that confirms a regression (bench-compare.sh exit 1,
# verdict regression) is a confirmed regression, not a failed re-run; only an error or no compare.json fails (exit 2)
BWT="$TMP/benchwt"
git init -q "$BWT" && git -C "$BWT" commit -q --allow-empty -m start
mkdir -p "$BWT/docs/perf/results" "$BWT/docs/perf/results-worst"
echo '{}' > "$BWT/docs/perf/results/v0.1.0.json"; echo '{}' > "$BWT/docs/perf/results-worst/v0.1.0.json"
: > "$BENCH_LOG"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" FAKE_RECHECK_EXIT=1 expect "bench-release.sh: a re-run that confirms a regression ends in exit 1 without --allow-perf-regression" 1 "performance regression: full(v0.1.0 -> v0.2.0" bench-release.sh "$BWT" 0.2.0 --note "$TMP/note.md"
check "the re-run was given an absolute --out, and its note section was written" 'grep -Eq -- "--out /.*/docs/perf/compare/.tmp-recheck-full" "$BENCH_LOG" && grep -Fq "**Confirmed**: ui.frame.x" "$TMP/note.md" && [ -f "$BWT/docs/perf/compare/v0.2.0-recheck.json" ]'
rm -f "$TMP/note.md"
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" FAKE_RECHECK_EXIT=1 expect "bench-release.sh: a re-run that confirms a regression ends in exit 0 with --allow-perf-regression" 0 "allowed by --allow-perf-regression" bench-release.sh "$BWT" 0.2.0 --allow-perf-regression --note "$TMP/note.md"
check "the note section is still written when the regression is allowed" 'grep -Fq "**Confirmed**: ui.frame.x" "$TMP/note.md"'
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-error.json" FAKE_RECHECK_EXIT=2 expect "bench-release.sh: a re-run that errors is exit 2, also with --allow-perf-regression" 2 "the re-run of the flagged full scenarios failed: the build failed" bench-release.sh "$BWT" 0.2.0 --allow-perf-regression
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/none.json" FAKE_RECHECK_EXIT=1 expect "bench-release.sh: a re-run without a compare.json is exit 2" 2 "the re-run of the flagged full scenarios failed: no compare.json" bench-release.sh "$BWT" 0.2.0

# spec: BUILD-034: without jq a time regression cannot be confirmed: it counts as it is (a PATH without jq)
NOJQ="$TMP/nojq"; mkdir "$NOJQ"
for t in bash sh git env sed awk sort seq paste tr head cp rm mkdir mktemp dirname cat date cut uname nproc grep printf sleep expr tail wc; do
  p=$(command -v "$t") && [ -x "$p" ] && ln -s "$p" "$NOJQ/$t"
done
cp "$TMP/bin/fake-shale" "$NOJQ/fake-shale"
check "the PATH for the next tests has no jq" '! PATH="$NOJQ" command -v jq >/dev/null'
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" PATH="$NOJQ" BENCH_BIN="$NOJQ/fake-shale" expect "bench-release.sh without jq: a time regression counts as it is, and stops the release" 1 "jq is missing, so a time regression cannot be confirmed" bench-release.sh "$BWT" 0.2.0
FAKE_COMPARE_JSON="$TMP/time-reg.json" FAKE_COMPARE_EXIT=1 FAKE_RECHECK_JSON="$TMP/recheck-real.json" PATH="$NOJQ" BENCH_BIN="$NOJQ/fake-shale" expect "bench-release.sh without jq: --allow-perf-regression lets it pass" 0 "allowed by --allow-perf-regression" bench-release.sh "$BWT" 0.2.0 --allow-perf-regression

# dev_045: the lines-of-code report that land.sh commits on top of the merge is part of the tag
cd "$TMP/work" && git checkout -q main && git fetch -q origin && git merge -q --ff-only origin/main
land pend45 docs/changelog-pending/I45.md $'### Fixed\n\n- Forty-five (#45).'
LAND_REPORT_CMD='printf "report of the tree\n" > docs/loc-report.md' expect "dev_045: a check tag lands with the report commit on top" 0 "pushed the tag to origin" make-tag.sh --issue 77 patch
TAG45="v$(version_on_main)"
check "dev_045: the tag is on the report commit, the tip of origin/main, which sits on the merge of the bump" 'git fetch -q origin && git log -1 --format=%s origin/main | grep -Fq "no-issue: lines-of-code report for release/v" && [ "$(git rev-parse "$TAG45^{commit}")" = "$(git rev-parse origin/main)" ] && [ "$(git rev-list --parents -1 origin/main^ | wc -w)" = 3 ] && git show "$TAG45:docs/loc-report.md" | grep -Fq "report of the tree"'

[ "$FAILED" = 0 ] && echo "all $N checks passed"
exit "$FAILED"

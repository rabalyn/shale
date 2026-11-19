#!/bin/bash
# Shared by scripts/make-tag.sh (check tags, FORGE-013) and scripts/make-release.sh (releases,
# FORGE-014), ADR-0018: work out the version, bump it on a branch from `origin/main`, land the
# branch with scripts/land.sh, tag the landed merge and push the tag to Forgejo, whose CI then
# checks it and, when green, sends `main` (and a release tag) on to GitHub (BUILD-016).
# Sourced, not run. Environment, for scripts/test-release.sh: RELEASE_REMOTE (default origin),
# RELEASE_GH (the GitHub CLI, default gh), RELEASE_REPO (default rabalyn/shale), RELEASE_DATE
# (the date of a changelog section, default today); LAND_* is passed on to land.sh. A release also
# runs scripts/bench-release.sh (BUILD-034): BENCH_SKIP=yes leaves it out, BENCH_ALLOW=yes lets a
# performance regression pass (make-release.sh --skip-bench, --allow-perf-regression).

REMOTE=${RELEASE_REMOTE:-origin}
GH=${RELEASE_GH:-gh}
GH_REPO=${RELEASE_REPO:-rabalyn/shale}
LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# take_land_lock: the lock of scripts/land.sh (<git dir>/land/lock), held until this script ends
# (the file descriptor closes with it), so no other landing slips in between the cut of the
# release branch and the tag (#348). A second landing or release waits, as land.sh does.
take_land_lock() {
  local common
  common=$(git rev-parse --path-format=absolute --git-common-dir)
  mkdir -p "$common/land"
  exec 8>"$common/land/lock"
  if ! flock -n 8; then
    echo "Another landing is running; waiting for it…"
    flock 8
  fi
}

# the worktree and branch of a run that has not landed yet, removed when the run stops
CLEAN_WT=""
CLEAN_BRANCH=""
cleanup() {
  if [ -n "$CLEAN_WT" ] && [ -e "$CLEAN_WT" ]; then git worktree remove --force "$CLEAN_WT" >/dev/null 2>&1 || rm -rf "$CLEAN_WT"; fi
  git worktree prune
  [ -z "$CLEAN_BRANCH" ] || git branch -q -D "$CLEAN_BRANCH" 2>/dev/null || true
  CLEAN_WT=""
  CLEAN_BRANCH=""
}

die() {
  echo "$(basename "$0"): $*" >&2
  cleanup
  exit 1
}

SEMVER='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'
is_semver() { [[ $1 =~ $SEMVER ]]; }
is_pre() { [[ $1 == *-* ]]; }
core() { echo "${1%%-*}"; }

# bump <patch|minor|major> <x.y.z>
bump() {
  local level=$1 major minor patch
  IFS=. read -r major minor patch <<< "$(core "$2")"
  case $level in
    patch) echo "$major.$minor.$((patch + 1))" ;;
    minor) echo "$major.$((minor + 1)).0" ;;
    major) echo "$((major + 1)).0.0" ;;
  esac
}

# the version in Cargo.toml of a commit
version_at() {
  git show "$1:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1
}

# the highest release tag (no pre-release part), without the v; empty when there is none
last_release() {
  git tag --list 'v*' | sed 's/^v//' | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1 || true
}

# set_version <folder> <version>: Cargo.toml's package version and the `shale` entry of Cargo.lock
set_version() {
  local dir=$1 v=$2
  sed -i "0,/^version = \".*\"/s//version = \"$v\"/" "$dir/Cargo.toml"
  awk -v v="$v" '
    /^name = "shale"$/ { on = 1; print; next }
    on && /^version = / { print "version = \"" v "\""; on = 0; next }
    { print }
  ' "$dir/Cargo.lock" > "$dir/Cargo.lock.new"
  mv "$dir/Cargo.lock.new" "$dir/Cargo.lock"
}

# release_issue <x.y.z>: the number of the open issue "Release x.y.z", made when there is none
release_issue() {
  local title="Release $1" n url
  n=$("$GH" issue list -R "$GH_REPO" --state open --search "\"$title\" in:title" --json number,title \
    -q ".[] | select(.title == \"$title\") | .number" | head -n 1) || die "gh could not list the issues"
  if [ -n "$n" ]; then
    echo "$n"
    return
  fi
  url=$("$GH" issue create -R "$GH_REPO" --title "$title" \
    --body "Check tags (\`scripts/make-tag.sh\`, FORGE-013) and the release (\`scripts/make-release.sh\`, FORGE-014) of $1 refer to this issue (ADR-0018). Close it when the release page is up.") \
    || die "gh could not create the issue \"$title\""
  echo "${url##*/}"
}

NEEDS_UPDATE="the changelog needs an update first: add entries to docs/changelog-pending/ (or to [Unreleased] in CHANGELOG.md) and land them"

# merge_changelog <folder> <version|""> <date>: [Unreleased] and docs/changelog-pending/*.md are
# merged by heading; with a version they become its section under a new empty [Unreleased]
# (a release, FORGE-014), without one they stay [Unreleased] (a check tag, FORGE-013). The
# pending files are removed with git rm.
merge_changelog() {
  local dir=$1 v=$2 date=$3 work cat f out
  local order=(Added Changed Deprecated Removed Fixed Security)
  work=$(mktemp -d)
  # the body of [Unreleased], then each pending file, cut into one file per heading
  awk '/^## \[/ { if (on) exit; if ($0 == "## [Unreleased]") { on = 1; next } } /^\[[^]]+\]: / { if (on) exit } on { print }' \
    "$dir/CHANGELOG.md" > "$work/0-unreleased"
  local sources=("$work/0-unreleased")
  if [ -d "$dir/docs/changelog-pending" ]; then
    while IFS= read -r f; do sources+=("$f"); done \
      < <(find "$dir/docs/changelog-pending" -maxdepth 1 -name '*.md' | sort -V)
  fi
  for f in "${sources[@]}"; do
    awk -v w="$work" -v src="${f#"$dir"/}" '
      function flush() { if (cat != "") { while (n > 0 && buf[n] ~ /^[[:space:]]*$/) n--; for (i = 1; i <= n; i++) print buf[i] >> (w "/cat-" cat); close(w "/cat-" cat) } n = 0 }
      /^### / { flush(); cat = substr($0, 5); gsub(/[^A-Za-z0-9 _-]/, "", cat); if (!(cat in seen)) { seen[cat] = 1; print cat >> (w "/order") } next }
      /^[[:space:]]*$/ && n == 0 { next }
      cat == "" { if (src ~ /0-unreleased$/) src = "the [Unreleased] section"; print "a changelog line outside a ### heading in " src ": " $0 > "/dev/stderr"; bad = 1; exit 1 }
      { buf[++n] = $0 }
      END { if (!bad) flush() }
    ' "$f" || { rm -rf "$work"; die "fix the changelog entry above; nothing was landed or tagged"; }
  done
  [ -s "$work/order" ] || { rm -rf "$work"; die "$NEEDS_UPDATE ([Unreleased] and docs/changelog-pending/ are empty)"; }
  local all=("${order[@]}")
  while IFS= read -r cat; do
    case " ${order[*]} " in *" $cat "*) ;; *) all+=("$cat") ;; esac
  done < <(awk '!s[$0]++' "$work/order")
  out="$work/section"
  if [ -n "$v" ]; then
    printf '## [Unreleased]\n\n## [%s] - %s\n' "$v" "$date" > "$out"
  else
    printf '## [Unreleased]\n' > "$out"
  fi
  for cat in "${all[@]}"; do
    [ -s "$work/cat-$cat" ] || continue
    printf '\n### %s\n\n' "$cat" >> "$out"
    cat "$work/cat-$cat" >> "$out"
  done
  printf '\n' >> "$out"
  # the new file: what is before [Unreleased], the new section, the older sections and the links
  awk -v sec="$out" -v v="$v" '
    $0 == "## [Unreleased]" { while ((getline l < sec) > 0) print l; skip = 1; next }
    skip && /^## \[/ { skip = 0 }
    skip && /^\[[^]]+\]: / { skip = 0 }
    skip { next }
    v != "" && /^\[Unreleased\]: .*\/compare\/.*\.\.\.HEAD$/ {
      base = $0; sub(/^\[Unreleased\]: /, "", base); sub(/\/compare\/.*/, "", base)
      prev = $0; sub(/.*\/compare\//, "", prev); sub(/\.\.\.HEAD$/, "", prev)
      print "[Unreleased]: " base "/compare/v" v "...HEAD"
      print "[" v "]: " base "/compare/" prev "...v" v
      next
    }
    { print }
  ' "$dir/CHANGELOG.md" > "$dir/CHANGELOG.md.new"
  mv "$dir/CHANGELOG.md.new" "$dir/CHANGELOG.md"
  if [ -d "$dir/docs/changelog-pending" ] && [ -n "$(find "$dir/docs/changelog-pending" -maxdepth 1 -name '*.md' -print -quit)" ]; then
    git -C "$dir" rm -q -- 'docs/changelog-pending/*.md'
  fi
  rm -rf "$work"
}

# has_entries <commit>: [Unreleased] of CHANGELOG.md or a file of docs/changelog-pending/ has a line
has_entries() {
  [ -n "$(git ls-tree --name-only "$1" docs/changelog-pending/ | grep '\.md$' || true)" ] && return 0
  git show "$1:CHANGELOG.md" 2>/dev/null | awk '/^## \[/ { if (on) exit; if ($0 == "## [Unreleased]") { on = 1; next } } /^\[[^]]+\]: / { if (on) exit } on && /[^[:space:]]/ && !/^### / { found = 1 } END { exit !found }'
}

# tag_and_push <kind: check|release> <version> <issue|""> <dry-run: yes|no>
# The whole run of FORGE-013 / FORGE-014 once the version is known.
tag_and_push() {
  local kind=$1 v=$2 issue=$3 dry=$4 tag="v$2" common wt branch base bump_commit merge msg
  is_semver "$v" || die "$v is not a semver version (x.y.z or x.y.z-rc.n)"
  if [ "$kind" = check ]; then
    is_pre "$v" || die "a check tag is a pre-release (x.y.z-rc.n); a release is scripts/make-release.sh"
  else
    is_pre "$v" && die "a release has no pre-release part; a check tag is scripts/make-tag.sh"
  fi
  git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "the tag $tag exists already (a pushed tag is never moved: take the next version)"
  [ -z "$(git ls-remote --tags "$REMOTE" "refs/tags/$tag")" ] || die "the tag $tag exists on $REMOTE already"
  local main prev
  main=$(git rev-parse "refs/remotes/$REMOTE/main")
  if [ "$(version_at "$main")" = "$v" ]; then
    # the bump of an earlier run landed but was not tagged (its check failed, #348): tag it now
    echo "$kind tag: $tag (the version $v is on $REMOTE/main already, an earlier run landed it without a tag; this run checks and tags $(git rev-parse --short "$main"))"
    if [ "$dry" = yes ]; then
      echo "(--dry-run: nothing was changed)"
      return 0
    fi
    [ -n "$issue" ] || issue=$(release_issue "$(core "$v")")
    [[ $issue =~ ^[0-9]+$ ]] || die "the release issue \"$issue\" is not a number"
    finish_tag "$kind" "$v" "$issue" "$main"
    return 0
  fi
  if [ "$kind" = check ]; then
    # a check tag needs a changelog change since the previous tag
    prev=$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$main" 2>/dev/null || true)
    if [ -n "$prev" ] && git diff --quiet "$prev" "$main" -- CHANGELOG.md docs/changelog-pending; then
      die "$NEEDS_UPDATE (nothing changed in them since $prev)"
    fi
  fi
  has_entries "$main" || die "$NEEDS_UPDATE ([Unreleased] and docs/changelog-pending/ are empty)"
  echo "$kind tag: $tag"
  if [ "$dry" = yes ]; then
    echo "(--dry-run: nothing was changed)"
    return 0
  fi

  [ -n "$issue" ] || issue=$(release_issue "$(core "$v")")
  [[ $issue =~ ^[0-9]+$ ]] || die "the release issue \"$issue\" is not a number"
  echo "release issue: #$issue"

  common=$(git rev-parse --path-format=absolute --git-common-dir)
  wt="$common/release/worktree"
  branch="release/$tag"
  base=$(git rev-parse "refs/remotes/$REMOTE/main")
  git rev-parse -q --verify "refs/heads/$branch" >/dev/null && die "the branch $branch exists (an earlier run that did not land?); delete it or land it first"
  mkdir -p "$common/release"
  if [ -e "$wt" ]; then git worktree remove --force "$wt" >/dev/null 2>&1 || rm -rf "$wt"; fi
  git worktree prune
  git worktree add -q -b "$branch" "$wt" "$base"
  CLEAN_WT=$wt
  CLEAN_BRANCH=$branch
  set_version "$wt" "$v"
  if [ "$kind" = release ]; then
    merge_changelog "$wt" "$v" "${RELEASE_DATE:-$(date +%F)}"
    # the performance of the release: stored, compared with the last release, and in the release commit (BUILD-034)
    if [ "${BENCH_SKIP:-no}" != yes ]; then
      bash "$LIB_DIR/bench-release.sh" "$wt" "$v" ${BENCH_ALLOW:+--allow-perf-regression} \
        || die "the performance step failed (see above); nothing was landed or tagged"
    else
      echo "note: --skip-bench: the performance of $v is not measured"
    fi
    msg="Release $v (Refs #$issue)"
  else
    merge_changelog "$wt" "" ""
    msg="Version $v (check tag, Refs #$issue)"
  fi
  git -C "$wt" add -A
  git -C "$wt" commit -q -m "$msg" -m "Made by scripts/make-$([ "$kind" = check ] && echo tag || echo release).sh (FORGE-01$([ "$kind" = check ] && echo 3 || echo 4), ADR-0018)."
  # BUILD-013 before anything is landed: the same check Forgejo and GitHub run on the tag
  (cd "$wt" && sh .github/check-release.sh "$v") || die "the release check failed; nothing was landed or tagged"
  bump_commit=$(git -C "$wt" rev-parse HEAD)
  git worktree remove --force "$wt"
  CLEAN_WT=""

  # the lock is ours (take_land_lock): land.sh must not wait for it, but only when we are its parent
  if ! SHALE_LAND_LOCK_HELD=$$ bash "$LIB_DIR/land.sh" --release "$branch"; then
    die "landing the version bump failed, so main itself does not pass the checks (or it moved meanwhile); nothing was tagged. Fix main (a branch, landed) and run this again."
  fi

  git fetch -q "$REMOTE"
  merge=$(git log --merges --format='%H %P' "$base..refs/remotes/$REMOTE/main" | awk -v c="$bump_commit" '$3 == c { print $1 }' | head -n 1)
  [ -n "$merge" ] || die "the landed merge of $branch is not on $REMOTE/main"
  finish_tag "$kind" "$v" "$issue" "$merge"
}

# finish_tag <kind> <version> <issue> <commit>: checks the commit that is on `origin/main` as the
# CI will (BUILD-013, in a worktree of it), and only then tags it and pushes the tag (#348). A
# release also needs docs/changelog-pending/ empty in it. When something is missing nothing is
# tagged and the way out is printed: the version is on main already, so the entry is added in a
# branch, landed, and the script run again for the same version (it then tags `origin/main`).
finish_tag() {
  local kind=$1 v=$2 issue=$3 merge=$4 tag="v$2" common chk landed out pending="" problems=""
  landed=$(version_at "$merge")
  [ "$landed" = "$v" ] || die "$merge has version $landed, not $v; nothing was tagged"
  common=$(git rev-parse --path-format=absolute --git-common-dir)
  chk="$common/release/check"
  mkdir -p "$common/release"
  if [ -e "$chk" ]; then git worktree remove --force "$chk" >/dev/null 2>&1 || rm -rf "$chk"; fi
  git worktree prune
  git worktree add -q --detach "$chk" "$merge"
  CLEAN_WT=$chk
  out=$(cd "$chk" && sh .github/check-release.sh "$v" 2>&1) || problems=$out
  if [ "$kind" = release ]; then
    pending=$(git ls-tree --name-only "$merge" docs/changelog-pending/ | command grep '\.md$' || true)
    [ -z "$pending" ] || problems="$problems
docs/changelog-pending/ still has files in the landed commit (landed while this ran?): $(tr '\n' ' ' <<< "$pending")"
  fi
  if [ -n "$problems" ]; then
    echo "$problems" >&2
    echo >&2
    echo "$(basename "$0"): $tag was NOT tagged: the landed commit $(git rev-parse --short "$merge") on $REMOTE/main does not pass the release check (BUILD-013)." >&2
    echo "The version $v is on $REMOTE/main now. To recover, in a branch from $REMOTE/main add what is missing to the section of $v in CHANGELOG.md" >&2
    echo "(and git rm what is left in docs/changelog-pending/), land it with scripts/land.sh, then run $(basename "$0") $v again: it tags $REMOTE/main." >&2
    cleanup
    exit 1
  fi
  git worktree remove --force "$chk"
  CLEAN_WT=""
  if [ "$kind" = release ]; then
    git tag -a "$tag" -m "Shale $v" "$merge"
  else
    git tag -a "$tag" -m "Shale $v (check tag, FORGE-013)" "$merge"
  fi
  git push -q "$REMOTE" "refs/tags/$tag"
  cleanup
  echo "tagged $(git rev-parse --short "$merge") as $tag and pushed the tag to $REMOTE"
  "$GH" issue comment "$issue" -R "$GH_REPO" --body "Tagged \`$tag\` on \`$(git rev-parse --short "$merge")\` (scripts/make-$([ "$kind" = check ] && echo tag || echo release).sh); Forgejo CI checks it now." >/dev/null \
    || echo "note: gh could not comment on #$issue"
  echo
  echo "Forgejo CI runs for $tag now: fj --host git.home.rabalyn.de actions tasks"
  if [ "$kind" = release ]; then
    echo "When it is green, its mirror job pushes main and $tag to GitHub, which publishes the release."
    echo "When the release page is up: $GH issue close $issue -R $GH_REPO -c \"Released $tag\""
  else
    echo "When it is green, its mirror job pushes main to GitHub; the tag stays on Forgejo."
  fi
  echo "A red run: fix it in a branch, land it and make the next tag (a pushed tag is never moved)."
}

# preflight: the folder this runs in has no uncommitted changes (they would not be in the tag)
preflight() {
  [ -z "$(git status --porcelain --untracked-files=no)" ] || die "this checkout has uncommitted changes: commit and land them first, or run from a clean checkout"
  git fetch -q --tags "$REMOTE"
}

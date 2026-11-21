#!/bin/bash
# Tests scripts/bench-tests-wanted.sh (spec BUILD-035, #449) on a throwaway repository: which
# changed files make a landing run the benchmark tests.
#   bash scripts/test-bench-wanted.sh
set -u
SCRIPT="$(cd "$(dirname "$0")" && pwd)/bench-tests-wanted.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
export GIT_CONFIG_GLOBAL="$TMP/gitconfig"
git config --global user.email t@example.com
git config --global user.name test
git config --global commit.gpgsign false
git config --global init.defaultBranch main

# the script finds the repository from its own folder, so it runs from a copy in the throwaway one
git init -q "$TMP/repo"
cd "$TMP/repo" || exit 1
mkdir scripts
cp "$SCRIPT" scripts/bench-tests-wanted.sh
git add -A && git commit -q -m start

# case <wanted: yes|no> <file>…: one commit that changes the files, and the answer for it
case_() {
  local want=$1 got
  shift
  for f in "$@"; do mkdir -p "$(dirname "$f")"; printf '# %s\n' "$RANDOM" >> "$f"; done
  git add -A && git commit -q -m change
  got=$(sh scripts/bench-tests-wanted.sh HEAD~1 HEAD)
  N=$((N + 1))
  if [ "$got" = "$want" ]; then echo "ok $N - $want: $*"; else echo "not ok $N - $want expected, got '$got': $*"; FAILED=1; fi
}

case_ yes src/bench/runner.rs
case_ yes src/bench/sub/deep.rs
case_ yes src/main.rs
case_ yes benches/x.rs
case_ yes scripts/bench.sh
case_ yes scripts/bench-compare.sh
case_ yes scripts/bench-tests-wanted.sh
case_ yes Cargo.toml
case_ yes Cargo.lock
case_ yes rust-toolchain.toml
case_ yes scripts/land.sh
case_ yes docs/a.md src/bench/mod.rs
case_ no src/editor/mod.rs
case_ no src/mainly.rs
case_ no src/benchmark_notes.rs
case_ no docs/spec/engineering.md README.md
case_ no scripts/test-land.sh scripts/make-tag.sh
case_ no tests/bench_scripts.rs
case_ no docs/Cargo.toml

exit "$FAILED"

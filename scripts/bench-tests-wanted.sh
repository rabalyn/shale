#!/bin/sh
# Whether a change needs the benchmark tests (`cargo test --features bench bench::`, BUILD-035,
# #449). Usage: bench-tests-wanted.sh <base> <head>; prints "yes" or "no" for the change from
# <base> to <head>. Yes when it touches the harness or what defines the build: `src/bench/`,
# `src/main.rs` (the `bench` module and its options), `benches/`, `scripts/bench*.sh` (this script
# too), `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and `scripts/land.sh`. scripts/land.sh runs
# the tests on yes and always with --release; the bench clippy step runs on every landing, so a
# change elsewhere that breaks the harness' build is still caught.
set -eu
cd "$(dirname "$0")/.."
if git diff --name-only "$1" "$2" | grep -Eq '^(src/bench/|src/main\.rs$|benches/|scripts/bench[^/]*\.sh$|Cargo\.toml$|Cargo\.lock$|rust-toolchain\.toml$|scripts/land\.sh$)'; then
  echo yes
else
  echo no
fi

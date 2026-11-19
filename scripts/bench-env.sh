# Sourced by scripts/bench.sh, bench-compare.sh and bench-release.sh (BUILD-033): how the benchmark
# binary is built, the same everywhere so that results can be compared. Frame pointers make the
# stacks of a flamegraph complete (perf may only walk them, not unwind with DWARF, as an unprivileged
# user); they cost a percent or two.
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C force-frame-pointers=yes"

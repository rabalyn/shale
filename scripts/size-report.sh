#!/bin/bash
# The binary size report (BUILD-045, #697): what occupies the Linux release binary. Builds the
# profile `size-report` (the release profile with the symbols kept, no LTO, so the linker map can
# attribute every byte to a crate) with a linker map and runs `examples/size_report.rs` on it. It
# writes docs/size-report.md, docs/size-pie.svg, docs/size-history.tsv and docs/size-history.svg
# in the folder it measures; the history has one row per version. scripts/make-tag.sh and
# scripts/make-release.sh run it before the version commit (FORGE-013, FORGE-014), so a tag carries
# the report of its version.
#
#   scripts/size-report.sh [--root DIR] [--date YYYY-MM-DD] [VERSION]
#
# VERSION defaults to the one in Cargo.toml, the date to today. --root is the checkout to measure
# (default: the one this script is in). CARGO_TARGET_DIR is respected (default: target/ of the
# checkout); the map is kept in `<target>/size-report-files/`, so a second run with nothing changed
# only parses it. Needs Linux (lld, the default linker of rustc 1.100) and, for the size of the
# stripped binary, `strip`.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DATE=$(date +%F)
VERSION=""
while [ $# -gt 0 ]; do
  case $1 in
    --root) shift; ROOT=$(cd "${1:?--root needs a folder}" && pwd) ;;
    --date) shift; DATE=${1:?--date needs a date} ;;
    -h|--help) sed -n '2,/^set -euo/{/^#/p}' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "size-report.sh: unknown option $1" >&2; exit 2 ;;
    *) [ -z "$VERSION" ] || { echo "size-report.sh: one version only" >&2; exit 2; }; VERSION=$1 ;;
  esac
  shift
done
[ "$(uname -s)" = Linux ] || { echo "size-report.sh: the report measures the Linux build; this is $(uname -s)" >&2; exit 1; }
cd "$ROOT"
[ -n "$VERSION" ] || VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET=${CARGO_TARGET_DIR:-target}
mkdir -p "$TARGET"
TARGET=$(cd "$TARGET" && pwd)
export CARGO_TARGET_DIR=$TARGET
DIR=$TARGET/size-report-files
MAP=$DIR/shale.map
BIN=$TARGET/size-report/shale
mkdir -p "$DIR"
# cargo does not link again when nothing changed, so a deleted map is not written again: make it
[ -e "$MAP" ] || cargo clean -q -p shale --profile size-report
echo "size report: building the profile size-report with a linker map ($MAP)"
cargo rustc --profile size-report --locked --bin shale -- -C "link-arg=-Wl,-Map=$MAP"
[ -s "$MAP" ] || { echo "size-report.sh: the linker made no map $MAP" >&2; exit 1; }
STRIPPED=()
if command -v strip >/dev/null 2>&1; then
  strip -o "$DIR/shale.stripped" "$BIN"
  STRIPPED=(--stripped "$DIR/shale.stripped")
fi
cargo run -q --locked --example size_report -- --root "$ROOT" --map "$MAP" --bin "$BIN" ${STRIPPED[@]+"${STRIPPED[@]}"} \
  --version "$VERSION" --date "$DATE" --out docs/size-report.md
echo "size report: docs/size-report.md for $VERSION"

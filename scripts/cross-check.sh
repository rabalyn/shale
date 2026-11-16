#!/bin/sh
# Checks the macOS and Windows code (`cfg`) on a Linux machine before it is pushed (DEV-015):
# clippy for both targets, and with --wine the Windows tests under Wine. GitHub (BUILD-017) stays
# the real check: it builds with MSVC, runs real Windows, and runs the macOS tests.
#
#   sh scripts/cross-check.sh                  clippy for Windows and macOS
#   sh scripts/cross-check.sh windows          clippy for Windows only (or: macos)
#   sh scripts/cross-check.sh --wine           also the Windows tests under Wine
#   sh scripts/cross-check.sh --wine windows   Windows clippy and tests
#
# Needs: the rustup targets x86_64-pc-windows-gnu and aarch64-apple-darwin; mingw-w64-gcc for
# Windows; zig for macOS; wine for --wine. Wine runs in its own prefix, SHALE_WINEPREFIX
# (default ~/.cache/shale-wine). Tests that fail only under Wine are skipped by name from
# scripts/wine-known-failures.txt.
set -u
cd "$(dirname "$0")/.." || exit 1
here=$(pwd)

wine=0
windows=0
macos=0
for a in "$@"; do
  case "$a" in
    --wine) wine=1 ;;
    windows) windows=1 ;;
    macos) macos=1 ;;
    -h | --help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown argument \"$a\" (see: sh scripts/cross-check.sh --help)." >&2; exit 2 ;;
  esac
done
if [ "$windows" = 0 ] && [ "$macos" = 0 ]; then
  windows=1
  macos=1
fi

# need <command> <what brings it>
missing=0
need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "\"$1\" is missing: install $2." >&2
    missing=1
  fi
}
need_target() {
  if ! rustup target list --installed 2>/dev/null | command grep -qx "$1"; then
    echo "The Rust target $1 is missing: rustup target add $1" >&2
    missing=1
  fi
}
if [ "$windows" = 1 ]; then
  need x86_64-w64-mingw32-gcc "mingw-w64-gcc (Arch) or gcc-mingw-w64-x86-64 (Debian, Ubuntu)"
  need_target x86_64-pc-windows-gnu
  if [ "$wine" = 1 ]; then need wine "wine"; fi
fi
if [ "$macos" = 1 ]; then
  need zig "zig"
  need_target aarch64-apple-darwin
fi
[ "$missing" = 0 ] || exit 1

failed=""
step() {
  name=$1
  shift
  echo "== $name"
  if "$@"; then
    echo "== $name: ok"
  else
    echo "== $name: FAILED"
    failed="$failed
  $name"
  fi
}

if [ "$windows" = 1 ]; then
  step "Windows clippy" cargo clippy --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings
fi
if [ "$macos" = 1 ]; then
  step "macOS clippy" env CC_aarch64_apple_darwin="$here/scripts/zig-cc-macos" AR_aarch64_apple_darwin="zig ar" \
    cargo clippy --all-targets --locked --target aarch64-apple-darwin -- -D warnings
fi

if [ "$wine" = 1 ] && [ "$windows" = 1 ]; then
  WINEPREFIX="${SHALE_WINEPREFIX:-$HOME/.cache/shale-wine}"
  export WINEPREFIX
  export WINEDEBUG=-all
  export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
  # The test folders are named after the process ID, and Wine gives every run the same one, so a
  # run would find the mails and files of the one before.
  rm -rf "$WINEPREFIX"/drive_c/users/*/AppData/Local/Temp/shale-*
  # the known Wine failures, as `--skip <name>` after `--exact`
  set -- --exact
  while read -r t _; do
    case "$t" in "" | "#"*) continue ;; esac
    set -- "$@" --skip "$t"
  done < scripts/wine-known-failures.txt
  step "Windows tests under Wine" cargo test --locked --target x86_64-pc-windows-gnu -- "$@"
fi

if [ -n "$failed" ]; then
  echo "Failed:$failed"
  exit 1
fi
echo "All cross checks passed."

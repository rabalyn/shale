#!/bin/sh
# Bumps packaging/aur/shale-bin (BUILD-026, raba/shale#337) to a released version: pkgver, pkgrel=1,
# the sha256 of the Linux download from the release's own .sha256 file, and a fresh .SRCINFO.
# Usage: scripts/aur-update.sh [VERSION]   (default: the version in Cargo.toml)
# It never touches the AUR: publishing is the owner's step, printed at the end.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dir=$root/packaging/aur/shale-bin
repo=rabalyn/shale

case "${1:-}" in
  -h | --help)
    echo "usage: scripts/aur-update.sh [VERSION]"
    exit 0
    ;;
esac

ver=${1:-}
if [ -z "$ver" ]; then
  ver=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
fi
case "$ver" in
  *[!0-9.]* | '')
    echo "error: '$ver' is not a released version like 1.2.3 (no pre-release part)" >&2
    exit 2
    ;;
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "error: '$ver' is not a version like 1.2.3" >&2
    exit 2
    ;;
esac

if ! command -v makepkg >/dev/null 2>&1; then
  echo "error: makepkg is needed to write .SRCINFO (run this on Arch Linux)" >&2
  exit 1
fi

file=shale-$ver-linux-x86_64.tar.gz.sha256
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

if command -v gh >/dev/null 2>&1; then
  gh release download "v$ver" -R "$repo" -p "$file" -D "$tmp" >/dev/null 2>&1 || true
fi
if [ ! -s "$tmp/$file" ] && command -v curl >/dev/null 2>&1; then
  curl -fsSL -o "$tmp/$file" "https://github.com/$repo/releases/download/v$ver/$file" 2>/dev/null || true
fi
if [ ! -s "$tmp/$file" ]; then
  echo "error: release v$ver or its checksum file $file was not found on GitHub ($repo)" >&2
  exit 1
fi

sum=$(awk '{print $1; exit}' "$tmp/$file")
case "$sum" in
  *[!0-9a-f]*) sum= ;;
esac
if [ "${#sum}" -ne 64 ]; then
  echo "error: $file does not start with a sha256 checksum" >&2
  exit 1
fi

sed -i -e "s/^pkgver=.*/pkgver=$ver/" -e "s/^pkgrel=.*/pkgrel=1/" -e "s/^sha256sums=.*/sha256sums=('$sum')/" "$dir/PKGBUILD"
(cd "$dir" && makepkg --printsrcinfo >.SRCINFO)

echo "shale-bin is now $ver-1 (sha256 $sum)."
echo "Next, to publish it on the AUR (your step, this script never does it):"
echo "  git clone ssh://aur@aur.archlinux.org/shale-bin.git /path/to/aur-shale-bin"
echo "  cp $dir/PKGBUILD $dir/.SRCINFO /path/to/aur-shale-bin/"
echo "  cd /path/to/aur-shale-bin && git add PKGBUILD .SRCINFO && git commit -m 'Update to $ver' && git push"

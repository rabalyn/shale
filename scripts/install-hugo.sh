#!/bin/sh
# Installs the Hugo that the tests of the site export build with (SITE-070, hugo_tests.rs): the
# release of the pinned version from GitHub, checked against the SHA-256 list of the same release.
# Used by CI, where the distribution's Hugo is too old for the theme (it needs 0.158 or newer).
#
#   scripts/install-hugo.sh [folder]     the folder for the program, /usr/local/bin if left out
#   scripts/install-hugo.sh --print-url  only prints the download URL for this machine's architecture
#                                        (SHALE_ARCH=aarch64 or x86_64 overrides `uname -m`)
#
# The release asset follows the machine: linux-amd64 on x86_64, linux-arm64 on aarch64 (the CI
# runner's job containers are arm64); any other architecture stops with a message (#683).
#
# Without Hugo the tests of the site export skip with a printed note (BUILD-018).
set -eu

VERSION=0.170.1
arch=${SHALE_ARCH:-$(uname -m)}
case "$arch" in
x86_64 | amd64) target=linux-amd64 ;;
aarch64 | arm64) target=linux-arm64 ;;
*)
    echo "install-hugo.sh: no Hugo download for the architecture '$arch' (x86_64 and aarch64 are supported)" >&2
    exit 1
    ;;
esac
name="hugo_extended_${VERSION}_${target}.tar.gz"
base="https://github.com/gohugoio/hugo/releases/download/v${VERSION}"
if [ "${1:-}" = "--print-url" ]; then
    echo "$base/$name"
    exit 0
fi
dest=${1:-/usr/local/bin}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/checksums.txt" "$base/hugo_${VERSION}_checksums.txt"
(cd "$tmp" && grep " $name\$" checksums.txt | sha256sum -c -)
tar -xzf "$tmp/$name" -C "$tmp" hugo
install -m 0755 "$tmp/hugo" "$dest/hugo"
"$dest/hugo" version

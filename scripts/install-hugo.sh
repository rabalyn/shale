#!/bin/sh
# Installs the Hugo that the tests of the site export build with (SITE-070, hugo_tests.rs): the
# release of the pinned version from GitHub, checked against the SHA-256 list of the same release.
# Used by CI, where the distribution's Hugo is too old for the theme (it needs 0.158 or newer).
#
#   scripts/install-hugo.sh [folder]     the folder for the program, /usr/local/bin if left out
#
# Without Hugo the tests of the site export skip with a printed note (BUILD-018).
set -eu

VERSION=0.170.1
dest=${1:-/usr/local/bin}
name="hugo_extended_${VERSION}_linux-amd64.tar.gz"
base="https://github.com/gohugoio/hugo/releases/download/v${VERSION}"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/checksums.txt" "$base/hugo_${VERSION}_checksums.txt"
(cd "$tmp" && grep " $name\$" checksums.txt | sha256sum -c -)
tar -xzf "$tmp/$name" -C "$tmp" hugo
install -m 0755 "$tmp/hugo" "$dest/hugo"
"$dest/hugo" version

#!/bin/sh
# Installs the Pagefind that the tests of the site export index a built site with (SITE-048,
# hugo_tests.rs) and that the server recipe of the user guide names (SITE-049): the release of the
# pinned version from the project's GitHub page, checked against the SHA-256 that is written here
# (so a changed file on the release page is not trusted) and against the SHA-256 file of the release.
# Used by CI next to scripts/install-hugo.sh.
#
#   scripts/install-pagefind.sh [folder]     the folder for the program, /usr/local/bin if left out
#   scripts/install-pagefind.sh --print-url  only prints the download URL for this machine's architecture
#                                            (SHALE_ARCH=aarch64 or x86_64 overrides `uname -m`)
#
# The release asset follows the machine: x86_64-unknown-linux-musl on x86_64, aarch64-unknown-linux-musl
# on aarch64 (the CI runner's job containers are arm64), each with its own pinned SHA-256; any other
# architecture stops with a message (#683).
#
# Without Pagefind the tests that run it skip with a printed note (BUILD-018).
set -eu

VERSION=1.6.0
arch=${SHALE_ARCH:-$(uname -m)}
case "$arch" in
x86_64 | amd64)
    target=x86_64-unknown-linux-musl
    sha256=004d40c9a644c995dadc42b8031196c6f17e7a6dacf9a6c15c63286dd6ead5f2
    ;;
aarch64 | arm64)
    target=aarch64-unknown-linux-musl
    sha256=ad286f25e3109a2c6c740bb46998f75e1eae93df41f64c4699f0c2bc8db101ce
    ;;
*)
    echo "install-pagefind.sh: no Pagefind download for the architecture '$arch' (x86_64 and aarch64 are supported)" >&2
    exit 1
    ;;
esac
name="pagefind-v${VERSION}-${target}.tar.gz"
base="https://github.com/Pagefind/pagefind/releases/download/v${VERSION}"
if [ "${1:-}" = "--print-url" ]; then
    echo "$base/$name"
    exit 0
fi
dest=${1:-/usr/local/bin}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/$name.sha256" "$base/$name.sha256"
echo "$sha256  $name" > "$tmp/pinned.sha256"
(cd "$tmp" && sha256sum -c pinned.sha256)
(cd "$tmp" && sed "s/^\([0-9a-f]*\) .*/\1  $name/" "$name.sha256" | sha256sum -c -)
tar -xzf "$tmp/$name" -C "$tmp" pagefind
install -m 0755 "$tmp/pagefind" "$dest/pagefind"
"$dest/pagefind" --version

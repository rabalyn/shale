#!/bin/sh
# Installs the Pagefind that the tests of the site export index a built site with (SITE-048,
# hugo_tests.rs) and that the server recipe of the user guide names (SITE-049): the release of the
# pinned version from the project's GitHub page, checked against the SHA-256 that is written here
# (so a changed file on the release page is not trusted) and against the SHA-256 file of the release.
# Used by CI next to scripts/install-hugo.sh (Forgejo on Linux, GitHub's main-check on macOS and
# Windows, BUILD-039).
#
#   scripts/install-pagefind.sh [folder]     the folder for the program, /usr/local/bin if left out
#
# The asset is chosen by system and CPU: Linux x86_64 (musl), macOS arm64 and x86_64, Windows
# (Git Bash) x86_64, which gives `pagefind.exe`. Without Pagefind the tests that run it skip with a
# printed note (BUILD-018).
set -eu

VERSION=1.6.0
dest=${1:-/usr/local/bin}
base="https://github.com/Pagefind/pagefind/releases/download/v${VERSION}"
exe=pagefind

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    target=x86_64-unknown-linux-musl
    sha256=004d40c9a644c995dadc42b8031196c6f17e7a6dacf9a6c15c63286dd6ead5f2
    ;;
  Darwin-arm64)
    target=aarch64-apple-darwin
    sha256=43f17bec7ad55947f06c8f9689a1ac2c5d1ea7c96294bceaa3cc4e7152c1cf55
    ;;
  Darwin-x86_64)
    target=x86_64-apple-darwin
    sha256=2459b1ab8b5d27375a4aedf856f1d6060e5508d18a769238d672c07eaca2fe96
    ;;
  MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64)
    target=x86_64-pc-windows-msvc
    sha256=11fc76e8efec23bae11b7100fbf596438b59cc6da2097cf63f3a8ced3802f270
    exe=pagefind.exe
    ;;
  *)
    echo "No Pagefind release is known for $(uname -s) $(uname -m)." >&2
    exit 1
    ;;
esac
name="pagefind-v${VERSION}-${target}.tar.gz"

# macOS has no sha256sum
if command -v sha256sum >/dev/null 2>&1; then
  check() { sha256sum -c -; }
else
  check() { shasum -a 256 -c -; }
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/$name.sha256" "$base/$name.sha256"
(cd "$tmp" && echo "$sha256  $name" | check)
(cd "$tmp" && sed "s/^\([0-9a-f]*\) .*/\1  $name/" "$name.sha256" | check)
tar -xzf "$tmp/$name" -C "$tmp" "$exe"
mkdir -p "$dest"
install -m 0755 "$tmp/$exe" "$dest/$exe"
"$dest/$exe" --version

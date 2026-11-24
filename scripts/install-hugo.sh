#!/bin/sh
# Installs the Hugo that the tests of the site export build with (SITE-070, hugo_tests.rs): the
# extended release of the pinned version from GitHub, checked against the SHA-256 that is written
# here (so a changed file on the release page is not trusted) and against the SHA-256 list of the
# same release. Used by CI (Forgejo on Linux, GitHub's main-check on macOS and Windows, BUILD-039),
# where the distribution's Hugo is too old for the theme (it needs 0.158 or newer).
#
#   scripts/install-hugo.sh [folder]     the folder for the program, /usr/local/bin if left out
#
# Linux (x86_64): the tar.gz; macOS: the universal .pkg, unpacked with `pkgutil` (Hugo publishes no
# tar.gz for macOS; nothing is installed system-wide); Windows (Git Bash, x86_64): the .zip, which
# gives `hugo.exe`. Without Hugo the tests of the site export skip with a printed note (BUILD-018).
set -eu

VERSION=0.170.1
dest=${1:-/usr/local/bin}
base="https://github.com/gohugoio/hugo/releases/download/v${VERSION}"

case "$(uname -s)" in
  Linux)
    name="hugo_extended_${VERSION}_linux-amd64.tar.gz"
    sha256=841a5462853aefc9942e4de1e8a5d75584b6a5181941e6ad3f1d745190e03cd8
    ;;
  Darwin)
    name="hugo_${VERSION}_darwin-universal.pkg"
    sha256=f7672f5e47f13c488ecd9ae96450ed9d2044629d60fe23570312e896c1918d73
    ;;
  MINGW* | MSYS* | CYGWIN*)
    name="hugo_extended_${VERSION}_windows-amd64.zip"
    sha256=e4a8bec642b5d18f451452fafb318bbb6b7f09e5208f3dca76a204c888c9e072
    ;;
  *)
    echo "No Hugo release is known for $(uname -s)." >&2
    exit 1
    ;;
esac

# macOS has no sha256sum
if command -v sha256sum >/dev/null 2>&1; then
  check() { sha256sum -c -; }
else
  check() { shasum -a 256 -c -; }
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/checksums.txt" "$base/hugo_${VERSION}_checksums.txt"
(cd "$tmp" && echo "$sha256  $name" | check)
(cd "$tmp" && grep " $name\$" checksums.txt | check)

mkdir -p "$dest"
exe=hugo
case "$name" in
  *.tar.gz)
    tar -xzf "$tmp/$name" -C "$tmp" hugo
    install -m 0755 "$tmp/hugo" "$dest/hugo"
    ;;
  *.pkg)
    pkgutil --expand-full "$tmp/$name" "$tmp/pkg"
    found=$(find "$tmp/pkg" -type f -name hugo | head -n 1)
    [ -n "$found" ] || { echo "No hugo program in $name." >&2; exit 1; }
    install -m 0755 "$found" "$dest/hugo"
    ;;
  *.zip)
    exe=hugo.exe
    if command -v unzip >/dev/null 2>&1; then
      unzip -q -o "$tmp/$name" hugo.exe -d "$tmp/zip"
    else
      7z x -y -o"$tmp/zip" "$tmp/$name" hugo.exe >/dev/null
    fi
    install -m 0755 "$tmp/zip/hugo.exe" "$dest/hugo.exe"
    ;;
esac
"$dest/$exe" version

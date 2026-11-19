#!/bin/sh
# Installs TeX Live inside the test image (TEX-020, ADR-0020), the way Shale's managed install does
# (TEX-002, `src/pdf/tex/install.rs`). Usage: install-texlive.sh <repository> <profile> <packages.txt>
#
# The repository is usually mirror.ctan.org, which sends every request to some CTAN mirror, and the
# mirrors are not all equally current: install-tl from a newer mirror followed by tlmgr from an older
# one fails with "… seems to be older than the local installation" (#320). So the repository is
# resolved ONCE to one concrete mirror, which install-tl and tlmgr both use; when tlmgr still finds
# its mirror older than the installation, a fresh mirror is chosen, three tlmgr runs at most.
# After the packages the font names database of luaotfload is built once, here, and the fonts the templates and
# the test document stand on are looked up in it (#327, #362): a container of this image runs every compile
# with a fresh --rm file system, so a database built at run time would be built by every compile again, slowly,
# with many at once, and the lookups of fonts like lmroman10-regular are where an incomplete install shows. When
# a font is not found, the font packages are installed again from a freshly resolved mirror and the database is
# built once more; when it is still not found, the build fails and names the mirrors.
# /etc/shale-tex-build records the TeX Live year, the repository, the mirror, the build date and the fonts.
set -eu
repo=${1%/}
profile=$2
packages=$3
probe=tlpkg/texlive.tlpdb.sha512
tries=3

case "$repo" in
  https://*) protos='=https' ;;
  http://*) protos='=http,https' ;;
  *) echo "$repo is not a repository address (https://…)" >&2; exit 1 ;;
esac

# the address the redirect for a small file of the repository ends at, without that file
resolve() {
  url=$(curl -fsSL --max-redirs 5 --proto "$protos" --proto-redir "$protos" -o /dev/null -w '%{url_effective}' "$repo/$probe") || {
    echo "the repository $repo could not be reached" >&2
    return 1
  }
  case "$url" in
    */"$probe") printf '%s\n' "${url%/"$probe"}" ;;
    *) echo "the repository $repo sent its $probe to $url, which is no TeX Live repository" >&2; return 1 ;;
  esac
}

mirror=$(resolve)
echo "TeX Live from $repo: mirror $mirror"

mkdir /tmp/install-tl
curl -fsSL --proto "$protos" "$mirror/install-tl-unx.tar.gz" | tar xz -C /tmp/install-tl --strip-components=1
/tmp/install-tl/install-tl --no-interaction --profile "$profile" --repository "$mirror"
rm -rf /tmp/install-tl
# only the folder of this machine's platform exists
PATH=/opt/texlive/bin/aarch64-linux:/opt/texlive/bin/x86_64-linux:$PATH

run=1
while :; do
  if sed 's/#.*//' "$packages" | xargs tlmgr --repository "$mirror" install >/tmp/tlmgr.log 2>&1; then
    cat /tmp/tlmgr.log
    break
  fi
  cat /tmp/tlmgr.log
  if [ "$run" -ge "$tries" ] || ! grep -q 'older than the local installation' /tmp/tlmgr.log; then
    exit 1
  fi
  run=$((run + 1))
  mirror=$(resolve)
  echo "that mirror is older than the installation: trying $mirror (run $run of $tries)" >&2
done
rm -f /tmp/tlmgr.log

# the fonts of the templates and of the test document (src/pdf/tex/compile.rs TEST_DOCUMENT): in the names of
# packages.txt, the packages of fonts are lm lm-math xcharter roboto (install::FONT_PACKAGES)
font_packages="lm lm-math xcharter roboto"
fonts="lmroman10-regular XCharter Roboto"

# builds the database from scratch and looks the fonts up in it; says which one it did not find
# (luaotfload-tool answers 0 whether it finds the font or not: its words are read)
build_fonts() {
  luaotfload-tool --update --force >/tmp/luaotfload.log 2>&1 || { cat /tmp/luaotfload.log; echo "luaotfload-tool could not build the font names database" >&2; return 1; }
  for font in $fonts; do
    luaotfload-tool --find="$font" 2>&1 | grep -q 'found!' || { echo "luaotfload-tool does not find the font $font" >&2; return 1; }
  done
}

# the font packages again from a freshly resolved mirror (in $fresh); a mirror older than the installation is left for
# another one, three runs at most, as above
reinstall_fonts() {
  fresh=$(resolve) || { echo "no fresh mirror could be chosen to install the fonts again" >&2; return 1; }
  fonts_run=1
  while :; do
    echo "installing $font_packages again from $fresh (the packages came from $mirror)" >&2
    # shellcheck disable=SC2086
    if tlmgr --repository "$fresh" install --reinstall $font_packages >/tmp/tlmgr-fonts.log 2>&1; then
      cat /tmp/tlmgr-fonts.log
      return 0
    fi
    cat /tmp/tlmgr-fonts.log
    if [ "$fonts_run" -ge "$tries" ] || ! grep -q 'older than the local installation' /tmp/tlmgr-fonts.log; then
      echo "the fonts could not be installed again from $fresh" >&2
      return 1
    fi
    fonts_run=$((fonts_run + 1))
    fresh=$(resolve) || return 1
  done
}

fonts_note="font names database built"
if ! build_fonts; then
  echo "the fonts are not found: the packages came incomplete from $mirror?" >&2
  reinstall_fonts || { echo "the fonts are not found and could not be installed again (the packages came from $mirror)" >&2; exit 1; }
  build_fonts || { echo "the fonts are still not found after installing them again from $fresh (the packages came from $mirror)" >&2; exit 1; }
  fonts_note="font names database built after the fonts were installed again from $fresh"
fi
rm -f /tmp/tlmgr-fonts.log rm -f /tmp/luaotfload.log

# which TeX Live this image is: year, repository, mirror, build date, fonts (`tex-env.sh status` prints it)
echo "TeX Live $(head -n 1 /opt/texlive/release-texlive.txt | sed 's/.*version //') from $repo (mirror $mirror), built $(date -u +%Y-%m-%d), $fonts_note" > /etc/shale-tex-build

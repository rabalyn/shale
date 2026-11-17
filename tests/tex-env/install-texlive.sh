#!/bin/sh
# Installs TeX Live inside the test image (TEX-020, ADR-0020), the way Shale's managed install does
# (TEX-002, `src/pdf/tex/install.rs`). Usage: install-texlive.sh <repository> <profile> <packages.txt>
#
# The repository is usually mirror.ctan.org, which sends every request to some CTAN mirror, and the
# mirrors are not all equally current: install-tl from a newer mirror followed by tlmgr from an older
# one fails with "… seems to be older than the local installation" (#320). So the repository is
# resolved ONCE to one concrete mirror, which install-tl and tlmgr both use; when tlmgr still finds
# its mirror older than the installation, a fresh mirror is chosen, three tlmgr runs at most.
# /etc/shale-tex-build records the TeX Live year, the repository, the mirror and the build date.
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

# which TeX Live this image is: year, repository, mirror, build date (`tex-env.sh status` prints it)
echo "TeX Live $(head -n 1 /opt/texlive/release-texlive.txt | sed 's/.*version //') from $repo (mirror $mirror), built $(date -u +%Y-%m-%d)" > /etc/shale-tex-build

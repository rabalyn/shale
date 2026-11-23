#!/bin/bash
# Signs the files of a published release with the owner's release key (UPD-022, ADR-0037).
# Run by the owner, by hand, on his own computer, AFTER GitHub has built and published the
# release (scripts/make-release.sh tags it, Forgejo's CI passes, GitHub builds the files and
# publishes the page). It is never part of land.sh, make-release.sh or CI, and the private key
# never leaves this computer.
#   scripts/sign-release.sh [--dry-run] [--resign] [--no-attestation] vX.Y.Z
#
# What it does:
#   1. checks that the key is the one built into Shale (src/update/release-signing.pub),
#   2. downloads every file of the release that has a `.sha256` file (the three downloads and the
#      two files of the user guide) with `gh release download`,
#   3. checks each against its `.sha256` and against the build attestation of the release
#      (`gh attestation verify`: built from this tag by this repository's workflow), so that
#      only what the workflow built is signed (not what someone else uploaded),
#   4. signs each with `ssh-keygen -Y sign -n shale-release/<file name>` (SSHSIG), checks the
#      signature with `ssh-keygen -Y verify`, and
#   5. uploads the `<file>.sig` files to the release with `gh release upload`.
# A file that already has a `.sig` on the release is left alone unless --resign.
# --dry-run does steps 1 to 3 and says what it would sign and upload; nothing is signed or uploaded.
#
# The key: $SHALE_SIGNING_KEY, default ~/.ssh/shale_github_signing (a passphrase is asked for by
# ssh-keygen once per file; to be asked once, `ssh-add` the key and point SHALE_SIGNING_KEY at the
# `.pub` file: ssh-keygen then signs through the agent). The built-in public key is
# $SHALE_RELEASE_PUB, default src/update/release-signing.pub (tests use another).
set -eu

REPO="rabalyn/shale"
SRC="$(cd "$(dirname "$0")/.." && pwd)"
KEY="${SHALE_SIGNING_KEY:-$HOME/.ssh/shale_github_signing}"
BUILTIN_PUB="${SHALE_RELEASE_PUB:-$SRC/src/update/release-signing.pub}"
DRY=0
RESIGN=0
ATTEST=1
TAG=""

die() { echo "error: $*" >&2; exit 1; }

for a in "$@"; do
  case "$a" in
    --dry-run) DRY=1 ;;
    --resign) RESIGN=1 ;;
    --no-attestation) ATTEST=0 ;;
    -*) die "unknown option $a (usage: scripts/sign-release.sh [--dry-run] [--resign] [--no-attestation] vX.Y.Z)" ;;
    *) [ -z "$TAG" ] || die "one version only"; TAG="$a" ;;
  esac
done
[ -n "$TAG" ] || die "usage: scripts/sign-release.sh [--dry-run] [--resign] [--no-attestation] vX.Y.Z"
case "$TAG" in v*) ;; *) TAG="v$TAG" ;; esac
printf '%s\n' "$TAG" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || die "$TAG is not a release version (vX.Y.Z; a check tag has no release)"

for tool in gh ssh-keygen; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is not installed"
done
[ -r "$KEY" ] || die "the signing key $KEY cannot be read (set SHALE_SIGNING_KEY)"
[ -r "$BUILTIN_PUB" ] || die "the public key built into Shale ($BUILTIN_PUB) cannot be read"

# 1. the key must be the one built into Shale, else the signatures would be refused by every Shale
case "$KEY" in
  *.pub) KEY_PUB="$KEY" ;;
  *) KEY_PUB="$KEY.pub" ;;
esac
[ -r "$KEY_PUB" ] || die "the public key $KEY_PUB next to the signing key cannot be read"
key_of() { awk 'NF >= 2 { print $1 " " $2; exit }' "$1"; }
[ "$(key_of "$KEY_PUB")" = "$(key_of "$BUILTIN_PUB")" ] || die "the key $KEY is not the release key built into Shale ($BUILTIN_PUB): Shale would refuse what it signs"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# the release must be published (GitHub builds it after the tag)
draft=$(gh release view "$TAG" -R "$REPO" --json isDraft -q .isDraft 2>/dev/null) || die "the release $TAG is not on GitHub yet: GitHub builds it after the tag is pushed and Forgejo's checks passed; run this again when the page is up"
[ "$draft" = false ] || die "the release $TAG is a draft: run this again when it is published"
gh release view "$TAG" -R "$REPO" --json assets -q '.assets[].name' > "$WORK/assets" || die "could not list the files of $TAG"

# every file that has a checksum file
FILES=$(sed -n 's/\.sha256$//p' "$WORK/assets")
[ -n "$FILES" ] || die "the release $TAG has no .sha256 files: is the release complete?"
for f in $FILES; do
  grep -qx "$f" "$WORK/assets" || die "the release $TAG has $f.sha256 but not $f"
done

# 2. and 3. download and check
BUNDLE=""
if [ "$ATTEST" = 1 ]; then
  BUNDLE=$(grep -E '\.sigstore\.json$' "$WORK/assets" | head -n 1 || true)
  [ -n "$BUNDLE" ] || die "the release $TAG has no build attestation (.sigstore.json); --no-attestation signs without checking it"
fi
TODO=""
for f in $FILES; do
  if grep -qx "$f.sig" "$WORK/assets" && [ "$RESIGN" = 0 ]; then
    echo "skip $f: it has a signature already (--resign replaces it)"
    continue
  fi
  TODO="$TODO $f"
done
[ -n "$TODO" ] || { echo "nothing to sign: every file of $TAG has a signature"; exit 0; }

get() { gh release download "$TAG" -R "$REPO" --dir "$WORK" --pattern "$1" || die "could not download $1"; }
[ -z "$BUNDLE" ] || get "$BUNDLE"
for f in $TODO; do
  echo "downloading $f"
  get "$f"
  get "$f.sha256"
  (cd "$WORK" && { sha256sum -c "$f.sha256" >/dev/null 2>&1 || shasum -a 256 -c "$f.sha256" >/dev/null 2>&1; }) || die "$f does not match its .sha256 file: nothing is signed"
  if [ -n "$BUNDLE" ]; then
    gh attestation verify "$WORK/$f" --bundle "$WORK/$BUNDLE" --repo "$REPO" \
      --signer-workflow "$REPO/.github/workflows/build.yml" --source-ref "refs/tags/$TAG" --deny-self-hosted-runners >/dev/null \
      || die "$f has no valid build attestation from $REPO's workflow for $TAG: nothing is signed"
  fi
  echo "checked $f"
done

if [ "$DRY" = 1 ]; then
  echo "dry run: would sign and upload:"
  for f in $TODO; do echo "  $f.sig"; done
  exit 0
fi

# 4. sign and check what was made
printf 'shale %s\n' "$(key_of "$BUILTIN_PUB")" > "$WORK/allowed_signers"
SIGS=""
for f in $TODO; do
  rm -f "$WORK/$f.sig"
  ssh-keygen -Y sign -q -f "$KEY" -n "shale-release/$f" "$WORK/$f" || die "signing $f failed"
  ssh-keygen -Y verify -f "$WORK/allowed_signers" -I shale -n "shale-release/$f" -s "$WORK/$f.sig" < "$WORK/$f" >/dev/null || die "the signature of $f does not verify: nothing is uploaded"
  SIGS="$SIGS $WORK/$f.sig"
  echo "signed $f"
done

# 5. upload
CLOBBER=""
[ "$RESIGN" = 0 ] || CLOBBER="--clobber"
# shellcheck disable=SC2086
gh release upload "$TAG" -R "$REPO" $CLOBBER $SIGS || die "uploading the signatures failed"
echo "uploaded the signatures to the release $TAG: Shale installs it by itself from now on"

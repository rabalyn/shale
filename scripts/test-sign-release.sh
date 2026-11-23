#!/bin/bash
# Tests scripts/sign-release.sh (UPD-025, UPD-022, ADR-0037) with a stub `gh` that serves a folder as the
# release and records its calls, and a throwaway ssh key standing in for the owner's: the real
# GitHub and the real key are never touched. Skipped (with a note) when ssh-keygen is missing.
#   bash scripts/test-sign-release.sh
set -u
if ! command -v ssh-keygen >/dev/null 2>&1; then
  echo "skipped: ssh-keygen is not installed"
  exit 0
fi
SRC="$(cd "$(dirname "$0")/.." && pwd)"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
ok() { N=$((N + 1)); echo "ok $N - $1"; }
nok() { N=$((N + 1)); FAILED=1; echo "not ok $N - $1"; }
unset SHALE_SIGNING_KEY SHALE_RELEASE_PUB

# a throwaway key is "the owner's", another one is a stranger's
ssh-keygen -q -t ed25519 -N "" -C owner -f "$TMP/owner"
ssh-keygen -q -t ed25519 -N "" -C stranger -f "$TMP/stranger"
export SHALE_RELEASE_PUB="$TMP/owner.pub"
export SHALE_SIGNING_KEY="$TMP/owner"

# the release: three downloads and a guide with their checksums, and the attestation bundle
REL="$TMP/release"
mkdir -p "$REL" "$TMP/bin"
for f in shale-1.2.3-linux-x86_64.tar.gz shale-1.2.3-macos-arm64.dmg shale-1.2.3-windows-x86_64.zip shale-userguide-1.2.3.zip; do
  printf 'contents of %s\n' "$f" > "$REL/$f"
  (cd "$REL" && sha256sum "$f" > "$f.sha256")
done
echo '{}' > "$REL/shale-1.2.3-linux-x86_64.tar.gz.sigstore.json"

cat > "$TMP/bin/gh" <<'GH'
#!/bin/bash
# the release 1.2.3 is the folder $REL; what is uploaded lands there; every call is logged
echo "$*" >> "$GH_LOG"
case "$1 $2" in
  "release view")
    case "$*" in
      *isDraft*) echo "${GH_DRAFT:-false}" ;;
      *assets*) ls "$REL" ;;
    esac
    [ "$3" = v1.2.3 ] || exit 1
    ;;
  "release download")
    pattern=""; dir=""
    while [ $# -gt 0 ]; do
      case "$1" in --pattern) pattern=$2; shift ;; --dir) dir=$2; shift ;; esac
      shift
    done
    cp "$REL/$pattern" "$dir/$pattern"
    # a download that was changed after the checksum was made
    [ -z "${GH_TAMPER:-}" ] || [ "$pattern" != "$GH_TAMPER" ] || echo "changed" >> "$dir/$pattern"
    ;;
  "release upload")
    for a in "$@"; do case "$a" in *.sig) cp "$a" "$REL/" ;; esac; done
    ;;
  "attestation verify")
    [ -z "${GH_NO_ATTESTATION:-}" ] || exit 1
    ;;
  *) exit 1 ;;
esac
GH
chmod +x "$TMP/bin/gh"
export PATH="$TMP/bin:$PATH" REL GH_LOG="$TMP/gh.log"
sign() { bash "$SRC/scripts/sign-release.sh" "$@"; }
reset() { rm -f "$REL"/*.sig "$GH_LOG"; unset GH_DRAFT GH_TAMPER GH_NO_ATTESTATION; export SHALE_SIGNING_KEY="$TMP/owner"; }
sigs() { n=0; for f in "$REL"/*.sig; do [ -e "$f" ] && n=$((n + 1)); done; echo "$n"; }
check() { if "${@:2}"; then ok "$1"; else nok "$1"; fi; }

# the files are signed, uploaded, and the signatures verify with the owner's public key in the namespace of the file
reset
if out=$(sign v1.2.3 2>&1) && [ "$(sigs)" = 4 ]; then ok "every file with a checksum gets a signature"; else nok "signing: $out"; fi
printf 'x %s\n' "$(cut -d' ' -f1,2 "$TMP/owner.pub")" > "$TMP/allowed"
good=1
for f in shale-1.2.3-linux-x86_64.tar.gz shale-1.2.3-macos-arm64.dmg shale-1.2.3-windows-x86_64.zip shale-userguide-1.2.3.zip; do
  ssh-keygen -Y verify -f "$TMP/allowed" -I x -n "shale-release/$f" -s "$REL/$f.sig" < "$REL/$f" >/dev/null 2>&1 || good=0
  # not valid for another file's namespace
  ssh-keygen -Y verify -f "$TMP/allowed" -I x -n "shale-release/other" -s "$REL/$f.sig" < "$REL/$f" >/dev/null 2>&1 && good=0
done
check "the signatures verify in the namespace of their file only" [ "$good" = 1 ]
check "the attestation bundle is not signed" [ ! -e "$REL/shale-1.2.3-linux-x86_64.tar.gz.sigstore.json.sig" ]
check "the build attestation was checked before signing" command grep -q 'attestation verify' "$GH_LOG"
check "the signatures were uploaded to the release of the repository" command grep -q 'release upload v1.2.3 -R rabalyn/shale' "$GH_LOG"

# a second run leaves signed files alone
if out=$(sign v1.2.3 2>&1) && echo "$out" | command grep -q 'nothing to sign'; then ok "a signed release is left alone"; else nok "second run: $out"; fi
# --resign replaces them, with --clobber
reset; sign v1.2.3 >/dev/null 2>&1
if sign --resign v1.2.3 >/dev/null 2>&1 && command grep -q -- '--clobber' "$GH_LOG"; then ok "--resign signs again and uploads with --clobber"; else nok "--resign"; fi

# --dry-run signs and uploads nothing
reset
if out=$(sign --dry-run v1.2.3 2>&1) && [ "$(sigs)" = 0 ] && ! command grep -q 'release upload' "$GH_LOG" && echo "$out" | command grep -q 'would sign'; then ok "--dry-run checks but signs and uploads nothing"; else nok "dry run: $out"; fi

# refusals: nothing is signed or uploaded
reset
export SHALE_SIGNING_KEY="$TMP/stranger"
if out=$(sign v1.2.3 2>&1); then nok "a key that is not the built-in one signed"; elif echo "$out" | command grep -q 'not the release key built into Shale' && [ "$(sigs)" = 0 ]; then ok "a key that is not the one built into Shale is refused"; else nok "stranger: $out"; fi
reset; export GH_TAMPER=shale-1.2.3-macos-arm64.dmg
if out=$(sign v1.2.3 2>&1); then nok "a download that does not match its checksum was signed"; elif echo "$out" | command grep -q 'does not match its .sha256' && [ "$(sigs)" = 0 ] && ! command grep -q 'release upload' "$GH_LOG"; then ok "a file that does not match its checksum stops everything"; else nok "tamper: $out"; fi
reset; export GH_NO_ATTESTATION=1
if out=$(sign v1.2.3 2>&1); then nok "a file without a valid attestation was signed"; elif echo "$out" | command grep -q 'no valid build attestation' && [ "$(sigs)" = 0 ]; then ok "a file without a valid build attestation is not signed"; else nok "attestation: $out"; fi
if sign --no-attestation v1.2.3 >/dev/null 2>&1 && [ "$(sigs)" = 4 ]; then ok "--no-attestation skips that check"; else nok "--no-attestation"; fi
reset; export GH_DRAFT=true
if out=$(sign v1.2.3 2>&1); then nok "a draft was signed"; elif echo "$out" | command grep -q 'is a draft'; then ok "a draft release is refused"; else nok "draft: $out"; fi
reset
if out=$(sign v9.9.9 2>&1); then nok "a release that does not exist was signed"; elif echo "$out" | command grep -q 'is not on GitHub yet'; then ok "a release that is not published yet is refused with the reason"; else nok "missing: $out"; fi
for bad in v1.2.3-rc.1 1.2 latest; do
  if out=$(sign "$bad" 2>&1); then nok "$bad was accepted"; elif echo "$out" | command grep -q 'not a release version'; then ok "$bad is refused"; else nok "$bad: $out"; fi
done
export SHALE_SIGNING_KEY="$TMP/missing"
if out=$(sign v1.2.3 2>&1); then nok "a missing key was accepted"; elif echo "$out" | command grep -q 'cannot be read'; then ok "a missing key is refused"; else nok "missing key: $out"; fi
if out=$(sign 2>&1); then nok "no version was accepted"; elif echo "$out" | command grep -q usage; then ok "no version prints the usage"; else nok "usage: $out"; fi

[ "$FAILED" = 0 ] || exit 1
echo "all $N passed"

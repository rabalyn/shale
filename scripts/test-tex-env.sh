#!/bin/bash
# Tests that `tests/tex-env/tex-env.sh build` passes `--build-arg TL_REPO=<url>` on to the container
# engine only when SHALE_TEX_ENV_REPOSITORY is set (spec DEV-025, #371). A copy of the script runs in a
# throwaway tree, with a fake engine that records its arguments and a fake curl and sha256sum, so
# nothing touches the network or the real cache.
#   bash scripts/test-tex-env.sh
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0
unset SHALE_TEX_ENV_REPOSITORY SHALE_TEX_ENV_IMAGE

mkdir -p "$TMP/tree/tests/tex-env" "$TMP/tree/assets/tex" "$TMP/bin"
cp "$ROOT/tests/tex-env/tex-env.sh" "$ROOT/tests/tex-env/Containerfile" "$ROOT/tests/tex-env/install-texlive.sh" "$TMP/tree/tests/tex-env/"
cp "$ROOT/assets/tex/texlive.profile" "$ROOT/assets/tex/packages.txt" "$TMP/tree/assets/tex/"
# the tuda-ci sources are "there": a fake curl and sha256sum, and the generated class already in the cache
sha=$(sed -n 's/^TUDA_SHA256=//p' "$ROOT/tests/tex-env/tex-env.sh")
mkdir -p "$TMP/tree/tests/tex-env/cache/tuda-ci/tex"
: > "$TMP/tree/tests/tex-env/cache/tuda-ci/tex/tudapub.cls"
cat > "$TMP/bin/curl" <<'CURL'
#!/bin/sh
while [ $# -gt 0 ]; do [ "$1" = -o ] && { : > "$2"; break; }; shift; done
CURL
printf '#!/bin/sh\necho "%s  $1"\n' "$sha" > "$TMP/bin/sha256sum"
cat > "$TMP/bin/engine" <<'ENGINE'
#!/bin/sh
echo "$*" >> "$CALLS"
ENGINE
chmod +x "$TMP/bin/curl" "$TMP/bin/sha256sum" "$TMP/bin/engine"

# run_build [args]: runs `tex-env.sh build [args]` and leaves the exit code in $RC, the engine's arguments in $CALLS_TEXT
run_build() {
  CALLS="$TMP/calls"
  export CALLS
  : > "$CALLS"
  PATH="$TMP/bin:$PATH" SHALE_TEX_ENV_ENGINE="$TMP/bin/engine" sh "$TMP/tree/tests/tex-env/tex-env.sh" build "$@" > "$TMP/out" 2>&1
  RC=$?
  CALLS_TEXT=$(cat "$CALLS")
}
check() {
  N=$((N + 1))
  if eval "$2"; then echo "ok $N - $1"; else echo "not ok $N - $1 (engine got: $CALLS_TEXT; output: $(cat "$TMP/out" 2>/dev/null))"; FAILED=1; fi
}

run_build
check "dev_025: without SHALE_TEX_ENV_REPOSITORY the build has no TL_REPO argument" '[ "$RC" = 0 ] && printf "%s" "$CALLS_TEXT" | grep -Fq "build -t shale-tex-env" && ! printf "%s" "$CALLS_TEXT" | grep -Fq TL_REPO'
SHALE_TEX_ENV_REPOSITORY=https://texlive.example.test/tlnet run_build
check "dev_025: with SHALE_TEX_ENV_REPOSITORY the build passes --build-arg TL_REPO=<url>" '[ "$RC" = 0 ] && printf "%s" "$CALLS_TEXT" | grep -Fq "build --build-arg TL_REPO=https://texlive.example.test/tlnet -t shale-tex-env"'
SHALE_TEX_ENV_REPOSITORY=https://texlive.example.test/tlnet run_build --refresh
check "dev_025: --refresh and the mirror go together" '[ "$RC" = 0 ] && printf "%s" "$CALLS_TEXT" | grep -Fq "build --no-cache --build-arg TL_REPO=https://texlive.example.test/tlnet -t shale-tex-env"'

exit "$FAILED"

#!/bin/sh
# Helper for Shale's TeX test environment. Usage: ./tex-env.sh build | status | compile <dir> <file.tex> | reset
#   build    fetch the tuda-ci sources (checked by sha256), build the TeX Live image, and generate the
#            .cls/.sty files from the .dtx with docstrip (first time; later runs reuse all three)
#   status   say whether the image and the tuda-ci files are there
#   compile  run latexmk (LuaLaTeX, biber) on <dir>/<file.tex> inside the container, <dir> mounted
#            as /work. TEXINPUTS is /work/tex// and BIBINPUTS /work; SHALE_TEX_PRE is TeX code put
#            before the file (the tuda-ci demos get graphicx' `demo` option this way)
#   reset    remove the image and the cache (the next build starts again)
# SHALE_TEX_ENV_ENGINE=docker selects Docker instead of Podman.
set -eu
cd "$(dirname "$0")"
root=$(pwd)

ENGINE=${SHALE_TEX_ENV_ENGINE:-podman}
IMAGE=shale-tex-env
# the pinned fixture: tuda-ci 4.06 (there is no tag for it), git commit and the sha256 of GitHub's archive
TUDA_COMMIT=e933be93b07008edc9ce77e33b877d15489a476f
TUDA_SHA256=477060aeb6e459d661baac1942e83a2bd45e7dd9cdfdff9c3c24f72006b5c6d3
TUDA_URL=https://github.com/tudace/tuda_latex_templates/archive/$TUDA_COMMIT.tar.gz
cache=$root/cache

image_there() { "$ENGINE" image exists "$IMAGE" 2>/dev/null || "$ENGINE" image inspect "$IMAGE" >/dev/null 2>&1; }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

case "${1:-}" in
  build)
    mkdir -p "$cache"
    if [ ! -f "$cache/tuda-ci.tar.gz" ] || [ "$(sha256_of "$cache/tuda-ci.tar.gz")" != "$TUDA_SHA256" ]; then
      curl -fsSL -o "$cache/tuda-ci.tar.gz" "$TUDA_URL"
      [ "$(sha256_of "$cache/tuda-ci.tar.gz")" = "$TUDA_SHA256" ] || { echo "tuda-ci archive: wrong sha256" >&2; rm -f "$cache/tuda-ci.tar.gz"; exit 1; }
    fi
    # the build context: the Containerfile with the profile and the package list of assets/tex/
    ctx=$cache/context
    rm -rf "$ctx"; mkdir -p "$ctx"
    cp Containerfile "$ctx/"
    cp ../../assets/tex/texlive.profile ../../assets/tex/packages.txt "$ctx/"
    "$ENGINE" build -t "$IMAGE" -f "$ctx/Containerfile" "$ctx"
    if [ ! -f "$cache/tuda-ci/tex/tudapub.cls" ]; then
      rm -rf "$cache/tuda-ci"; mkdir -p "$cache/tuda-ci"
      tar xzf "$cache/tuda-ci.tar.gz" -C "$cache/tuda-ci" --strip-components=1
      # docstrip makes the classes and packages from the .dtx next to them
      "$ENGINE" run --rm -v "$cache/tuda-ci/tex:/work:Z" "$IMAGE" latex -interaction=batchmode tuda-ci.ins >/dev/null
      [ -f "$cache/tuda-ci/tex/tudapub.cls" ] || { echo "docstrip made no tudapub.cls" >&2; exit 1; }
      # the TU logo cannot be redistributed (TEX-007): the classes look for tuda_logo.pdf while the
      # theme is loaded, before graphicx' demo option can draw a box, so a plain grey bar stands in
      "$ENGINE" run --rm -v "$cache/tuda-ci/tex:/work:Z" "$IMAGE" lualatex -interaction=batchmode '\documentclass{article}\usepackage[paperwidth=60mm,paperheight=20mm,margin=1mm]{geometry}\pagestyle{empty}\begin{document}\noindent\rule{58mm}{18mm}\end{document}' >/dev/null || true
      mv "$cache/tuda-ci/tex/texput.pdf" "$cache/tuda-ci/tex/tuda_logo.pdf"
      rm -f "$cache/tuda-ci/tex/texput.log"
    fi
    echo "TeX test environment is ready. See tests/tex-env/README.md."
    ;;
  status)
    if image_there; then echo "image $IMAGE: there"; else echo "image $IMAGE: missing (run: $0 build)"; fi
    if [ -f "$cache/tuda-ci/tex/tudapub.cls" ]; then echo "tuda-ci $TUDA_COMMIT: generated"; else echo "tuda-ci: missing (run: $0 build)"; fi
    ;;
  compile)
    [ $# -eq 3 ] || { echo "usage: $0 compile <dir> <file.tex>" >&2; exit 2; }
    dir=$(cd "$2" && pwd)
    "$ENGINE" run --rm -v "$dir:/work:Z" -e TEXINPUTS=/work/tex//: -e BIBINPUTS=/work: -e SHALE_TEX_PRE="${SHALE_TEX_PRE:-}" "$IMAGE" \
      sh -c 'latexmk -lualatex -interaction=nonstopmode -halt-on-error -usepretex -pretex="$SHALE_TEX_PRE" "$1"' sh "$3"
    ;;
  reset)
    "$ENGINE" rmi -f "$IMAGE" >/dev/null 2>&1 || true
    rm -rf "$cache"
    ;;
  *)
    echo "usage: $0 build | status | compile <dir> <file.tex> | reset" >&2
    exit 2
    ;;
esac

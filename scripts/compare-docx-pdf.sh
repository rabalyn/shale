#!/usr/bin/env bash
# Compare how Word and Shale print a document (DEV-024, #339).
#
#   scripts/compare-docx-pdf.sh <file.docx | note.md> [outdir]
#
# A .docx is rendered to PDF by LibreOffice (left) and imported by Shale and exported with the
# options the export dialog opens with (right); a .md note has no Word side, so it is only exported.
# Both PDFs are rasterised and put side by side, one PNG per page, in outdir (default
# ${TMPDIR:-/tmp}/shale-compare-<name>/), whose path is printed. Needs pdftoppm, ImageMagick or
# python3 with PIL, and for a .docx soffice and pandoc. Not a user feature: a developer tool.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <file.docx | note.md> [outdir]" >&2
    exit 2
fi
input=$(realpath "$1")
[ -f "$input" ] || { echo "no such file: $input" >&2; exit 2; }
name=$(basename "$input")
stem=${name%.*}
case "${name##*.}" in
    docx | DOCX) kind=docx ;;
    md | MD) kind=md ;;
    *) echo "expected a .docx or a .md file: $name" >&2; exit 2 ;;
esac
safe=${stem//[^A-Za-z0-9._-]/_}
out=${2:-${TMPDIR:-/tmp}/shale-compare-$safe}
mkdir -p "$out"
out=$(realpath "$out")
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

need() { command -v "$1" > /dev/null 2>&1 || { echo "$1 is not installed ($2)" >&2; exit 1; }; }
need pdftoppm "poppler-utils"
if command -v magick > /dev/null 2>&1; then
    joiner=magick
elif command -v convert > /dev/null 2>&1; then
    joiner=convert
elif python3 -I -c 'import PIL' > /dev/null 2>&1; then
    joiner=pil
else
    echo "no ImageMagick (magick, convert) and no python3 with PIL: cannot join the pages" >&2
    exit 1
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/shale-compare-work.XXXXXX")
trap 'rm -rf "$work"' EXIT
rm -f "$out"/*.pdf "$out"/*.png

# the Word side: LibreOffice with a profile of its own, so a running LibreOffice does not matter
if [ "$kind" = docx ]; then
    need soffice "LibreOffice"
    cp "$input" "$work/word.docx"
    soffice "-env:UserInstallation=file://$work/profile" --headless --convert-to pdf --outdir "$work" "$work/word.docx" > /dev/null
    cp "$work/word.pdf" "$out/word.pdf"
fi

# the Shale side: an ignored test of the crate drives the real import and export (no window)
if [ "$kind" = docx ]; then
    export SHALE_COMPARE_DOCX=$input
else
    export SHALE_COMPARE_MD=$input
fi
export SHALE_COMPARE_OUT=$out/shale.pdf
(cd "$root" && cargo test --locked --quiet dev_024_compare_export -- --ignored)

# rasterise and join
pdftoppm -r 80 -png "$out/shale.pdf" "$work/s"
if [ "$kind" = docx ]; then
    pdftoppm -r 80 -png "$out/word.pdf" "$work/w"
fi
shopt -s nullglob
shale_pages=("$work"/s-*.png)
word_pages=("$work"/w-*.png)
count=${#shale_pages[@]}
if [ "${#word_pages[@]}" -gt "$count" ]; then count=${#word_pages[@]}; fi

join_pages() { # word-page-or-empty shale-page-or-empty target label-left label-right
    case "$joiner" in
        magick | convert)
            local left=$1 right=$2
            [ -n "$left" ] || left="xc:white"
            [ -n "$right" ] || right="xc:white"
            "$joiner" -background white "$left" "$right" -gravity center +append \
                -gravity north -splice 0x24 -pointsize 14 -fill black -annotate +0+4 "$4          |          $5" "$3"
            ;;
        pil)
            python3 -I - "$1" "$2" "$3" "$4" "$5" << 'PY'
import sys
from PIL import Image, ImageDraw
l, r, target, ll, rl = sys.argv[1:6]
imgs = [Image.open(p).convert("RGB") if p else None for p in (l, r)]
size = next(i.size for i in imgs if i)
imgs = [i or Image.new("RGB", size, "white") for i in imgs]
h = max(i.height for i in imgs)
sheet = Image.new("RGB", (imgs[0].width + imgs[1].width, h + 24), "white")
sheet.paste(imgs[0], (0, 24))
sheet.paste(imgs[1], (imgs[0].width, 24))
d = ImageDraw.Draw(sheet)
d.text((4, 4), ll, fill="black")
d.text((imgs[0].width + 4, 4), rl, fill="black")
sheet.save(target)
PY
            ;;
    esac
}

for ((i = 0; i < count; i++)); do
    page=$((i + 1))
    left=${word_pages[$i]:-}
    right=${shale_pages[$i]:-}
    if [ "$kind" = docx ]; then
        join_pages "$left" "$right" "$(printf '%s/page-%02d.png' "$out" "$page")" "Word (LibreOffice) page $page" "Shale page $page"
    else
        cp "$right" "$(printf '%s/page-%02d.png' "$out" "$page")"
    fi
done
echo "$count page(s); side-by-side PNGs and both PDFs are in $out"

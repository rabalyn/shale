#!/bin/sh
# Makes the Word files that the docx import tests use (IMP-039, DEV-004): every `.md` in
# src/testdata/docx/ becomes the `.docx` beside it, made by pandoc. The picture of picture.md
# is a tiny red square made here. The files are committed; run this only to change them.
#
#   sh scripts/make-docx-fixtures.sh
#
# styled.md is made with a reference document (made here from pandoc's own) that sets the page
# (Letter, margins 20, 25, 20, 30 mm), a page break before Heading 1, the body (Times New Roman 12 pt, justified, line spacing 1.15,
# 8 pt after), the colour of Heading 1 (2F5496) and a character style "Emphasis red" (7030A0), for
# the look tests (IMP-040, IMP-041); Heading 1 is bold there (#366).
#
# toc.docx (IMP-043) is made from toc.fodt, a flat ODT with a table of contents, by LibreOffice
# (`soffice --convert-to docx`). LibreOffice does not give the same bytes twice, so this step runs
# only when soffice is installed, and the committed toc.docx is what the test uses.
#
# Needs pandoc and python3 (for the picture and the reference document). The dates inside the
# files are fixed so that a second run gives the same files.
set -eu
cd "$(dirname "$0")/.." || exit 1
dir=src/testdata/docx
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export SOURCE_DATE_EPOCH=1700000000
python3 - "$tmp/pic.png" <<'PY'
import struct, sys, zlib
w, h = 16, 12
raw = b"".join(b"\x00" + b"\xd0\x20\x20" * w for _ in range(h))
def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
pandoc -o "$tmp/default.docx" --print-default-data-file reference.docx
python3 - "$tmp/default.docx" "$tmp/reference.docx" <<'PY'
import re, sys, zipfile
src, dst = sys.argv[1:]
zin = zipfile.ZipFile(src)
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as out:
    for info in zin.infolist():
        data = zin.read(info.filename)
        if info.filename == "word/styles.xml":
            s = data.decode("utf-8")
            # defaults: Times New Roman 12 pt, 1.15 lines, 8 pt after
            s = re.sub(r'<w:rFonts w:asciiTheme="minorHAnsi"[^>]*/>', '<w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman" />', s, count=1)
            s = s.replace('<w:spacing w:after="200" />', '<w:spacing w:after="160" w:line="276" w:lineRule="auto" />', 1)
            # Normal: justified
            s = s.replace('<w:name w:val="Normal" />\n    <w:qFormat />', '<w:name w:val="Normal" />\n    <w:qFormat />\n    <w:pPr>\n      <w:jc w:val="both" />\n    </w:pPr>', 1)
            # Body Text follows the defaults
            s = s.replace('<w:spacing w:before="180" w:after="180" />', '<w:spacing w:before="0" w:after="160" />', 1)
            # Heading 1: a fixed colour
            # (bold, for print-heading-weight, #366)
            s = re.sub(r'<w:color w:val="0F4761"[^>]*/>', '<w:b />\n        <w:color w:val="2F5496" />', s, count=1)
            # Heading 1 starts a page (print-break-before, IMP-043)
            s = s.replace('<w:outlineLvl w:val="0" />', '<w:pageBreakBefore />\n      <w:outlineLvl w:val="0" />', 1)
            # a character style with a colour, for the span "Emphasis red" of styled.md (IMP-041)
            style = '<w:style w:type="character" w:customStyle="1" w:styleId="EmphasisRed"><w:name w:val="Emphasis red" /><w:basedOn w:val="DefaultParagraphFont" /><w:qFormat /><w:rPr><w:color w:val="7030A0" /></w:rPr></w:style>'
            s = s.replace("</w:styles>", style + "</w:styles>", 1)
            data = s.encode("utf-8")
        elif info.filename == "word/document.xml":
            s = data.decode("utf-8")
            page = '<w:pgSz w:w="12240" w:h="15840" /><w:pgMar w:top="1134" w:right="1417" w:bottom="1134" w:left="1701" w:header="708" w:footer="708" w:gutter="0" />'
            s = s.replace("</w:footnotePr>", "</w:footnotePr>" + page, 1)
            data = s.encode("utf-8")
        zi = zipfile.ZipInfo(info.filename, (1980, 1, 1, 0, 0, 0))
        zi.compress_type = zipfile.ZIP_DEFLATED
        out.writestr(zi, data)
PY
for md in "$dir"/*.md; do
    case "$md" in
        */styled.md) reference="--reference-doc=$tmp/reference.docx" ;;
        *) reference="" ;;
    esac
    # shellcheck disable=SC2086
    pandoc "$md" -f markdown -t docx --resource-path="$tmp" $reference -o "${md%.md}.docx"
done
if command -v soffice >/dev/null 2>&1; then
    soffice --headless --convert-to docx --outdir "$tmp/toc" "$dir/toc.fodt" >/dev/null
    cp "$tmp/toc/toc.docx" "$dir/toc.docx"
else
    echo "soffice not found: toc.docx is left as it is"
fi
ls -l "$dir"

#!/bin/sh
# Makes the Word files that the docx import tests use (IMP-039, DEV-004): every `.md` in
# src/testdata/docx/ becomes the `.docx` beside it, made by pandoc. The picture of picture.md
# is a tiny red square made here. The files are committed; run this only to change them.
#
#   sh scripts/make-docx-fixtures.sh
#
# Needs pandoc and python3 (for the picture). The dates inside the files are fixed so that a
# second run gives the same files.
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
for md in "$dir"/*.md; do
    pandoc "$md" -f markdown -t docx --resource-path="$tmp" -o "${md%.md}.docx"
done
ls -l "$dir"

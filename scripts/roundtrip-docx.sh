#!/usr/bin/env bash
# Round trip of a Word file through Shale (DOCX-010, DEV-004).
#
#   scripts/roundtrip-docx.sh <file.docx> [out.docx]
#
# The Word file is imported into a new vault with the real pandoc (as the Import dialog does, the original
# kept), the note is exported to Word again, and both files are read back by pandoc as plain text. It prints
# the share of the words of the original that the export has again, and where both files are (out.docx
# defaults to ${TMPDIR:-/tmp}/shale-roundtrip-<name>.docx; the text of the note is written beside it, as .md). Needs pandoc. Not a user feature: a developer tool;
# open both files in Word or LibreOffice to look at what else changed. A step that fails prints `import error:` or
# `export error:` and the script ends with a failure. With SHALE_ROUNDTRIP_BIN set to a built test binary (what
# scripts/docx-corpus.sh does, to build once for many files) that binary is run instead of `cargo test`.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <file.docx> [out.docx]" >&2
    exit 2
fi
input=$(realpath "$1")
[ -f "$input" ] || { echo "no such file: $input" >&2; exit 2; }
name=$(basename "$input")
case "${name##*.}" in
    docx | DOCX) ;;
    *) echo "expected a .docx file: $name" >&2; exit 2 ;;
esac
command -v pandoc > /dev/null 2>&1 || { echo "pandoc is not installed" >&2; exit 1; }
stem=${name%.*}
safe=${stem//[^A-Za-z0-9._-]/_}
out=${2:-${TMPDIR:-/tmp}/shale-roundtrip-$safe.docx}
mkdir -p "$(dirname "$out")"
out=$(realpath -m "$out")
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

export SHALE_ROUNDTRIP_DOCX=$input
export SHALE_ROUNDTRIP_OUT=$out
if [ -n "${SHALE_ROUNDTRIP_BIN:-}" ]; then
    (cd "$root" && "$SHALE_ROUNDTRIP_BIN" dev_004_roundtrip --ignored --nocapture) | grep -E '^(words that came back|original|note|exported|import error|export error):'
else
    (cd "$root" && cargo test --locked --quiet dev_004_roundtrip -- --ignored --nocapture) | grep -E '^(words that came back|original|note|exported|import error|export error):'
fi

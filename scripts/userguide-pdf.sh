#!/bin/sh
# The user guide (docs/userguide, DEV-020) as one PDF, a book (BOOK-011, BOOK-012).
#   scripts/userguide-pdf.sh                    writes target/userguide.pdf
#   scripts/userguide-pdf.sh out/guide.pdf      writes that file
# SHALE=path/to/shale uses that program instead of `cargo run --release`.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/target/userguide.pdf}
test -f "$root/docs/userguide/Start here.md" || { echo "userguide-pdf.sh: no guide at $root/docs/userguide" >&2; exit 1; }
if [ -n "${SHALE:-}" ]; then
    "$SHALE" --export-pdf "$root/docs/userguide" "$out"
else
    cd "$root"
    cargo run --release --quiet -- --export-pdf docs/userguide "$out"
fi
echo "$out"

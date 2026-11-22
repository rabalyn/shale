#!/usr/bin/env bash
# A stress run of the Word import and export on real-world Word files (DEV-037, DOCX-010). A developer tool: it
# needs the network on the first run and is not part of land.sh.
#
#   scripts/docx-corpus.sh [--list FILE] [--only NAME] [--no-pages] [--keep DIR]
#
# Reads a pinned list (default scripts/docx-corpus.list: name, URL at a fixed commit, SHA-256, what the file
# covers), downloads the files into ${XDG_CACHE_HOME:-~/.cache}/shale-dev/docx-corpus/ (never into the repo; a file
# whose hash differs is an error), builds the test binary once and runs for each file
#   - the round trip of scripts/roundtrip-docx.sh (import into a new vault with the real pandoc, export,
#     words that came back),
#   - the schema check of scripts/validate-docx.sh --mce on the export (what Markup Compatibility lets a reader
#     ignore, such as `mc:Ignorable` and `w14:paraId`, which every file of Word has, is not an error) and on the
#     original, to tell what the original already breaks from what the export breaks,
#   - the page count of the original and of the export: wordcraft-cli when the flatpak ai.storyteller.wordcraft is
#     installed, else LibreOffice (`soffice --headless --convert-to pdf`, then pdfinfo), else no pages.
# and prints a table: name, words back, pages before -> after, schema result of the export (the failing parts, and
# those of the original), the import or export error. The exports stay in DIR (default ${TMPDIR:-/tmp}/shale-corpus).
# The exit status is the number of files with an error or a failing schema check of the export that the original
# does not have (at most 125).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
list=$root/scripts/docx-corpus.list
only=
pages=yes
keep=${TMPDIR:-/tmp}/shale-corpus
while [ $# -gt 0 ]; do
    case $1 in
        --list) list=${2:?--list needs a file}; shift ;;
        --only) only=${2:?--only needs a name}; shift ;;
        --no-pages) pages=no ;;
        --keep) keep=${2:?--keep needs a folder}; shift ;;
        -h | --help) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "docx-corpus.sh: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done
[ -f "$list" ] || { echo "docx-corpus.sh: no such list: $list" >&2; exit 2; }
for tool in pandoc curl sha256sum python3 xmllint unzip; do
    command -v "$tool" > /dev/null 2>&1 || { echo "docx-corpus.sh: $tool is not installed" >&2; exit 1; }
done

cache=${XDG_CACHE_HOME:-$HOME/.cache}/shale-dev/docx-corpus
mkdir -p "$cache" "$keep"
keep=$(realpath "$keep")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# the page counter
counter=none
if [ $pages = yes ]; then
    if command -v flatpak > /dev/null 2>&1 && flatpak info ai.storyteller.wordcraft > /dev/null 2>&1; then
        counter=wordcraft
    elif command -v soffice > /dev/null 2>&1 && command -v pdfinfo > /dev/null 2>&1; then
        counter=soffice
    fi
fi

page_count() {
    local file=$1 n=
    case $counter in
        wordcraft)
            n=$(flatpak run --filesystem="$(dirname "$file")" --command=wordcraft-cli ai.storyteller.wordcraft info "$file" 2> /dev/null \
                | python3 -c 'import json,sys; print(json.load(sys.stdin)["pages"])' 2> /dev/null) || n=
            ;;
        soffice)
            local out=$work/pdf
            rm -rf "$out" && mkdir -p "$out"
            timeout 120 soffice -env:UserInstallation="file://$work/profile" --headless --convert-to pdf --outdir "$out" "$file" > /dev/null 2>&1 || true
            n=$(pdfinfo "$out"/*.pdf 2> /dev/null | awk '/^Pages:/ {print $2}') || n=
            ;;
    esac
    echo "${n:-?}"
}

fetch() {
    local name=$1 url=$2 hash=$3 file=$cache/$1.docx
    if [ -f "$file" ] && [ "$(sha256sum "$file" | cut -d' ' -f1)" = "$hash" ]; then
        return 0
    fi
    curl -fsSL "$url" -o "$file.part" || { rm -f "$file.part"; echo "could not download $name" >&2; return 1; }
    if [ "$(sha256sum "$file.part" | cut -d' ' -f1)" != "$hash" ]; then
        rm -f "$file.part"
        echo "$name does not have the pinned SHA-256" >&2
        return 1
    fi
    mv "$file.part" "$file"
}

# the number of parts of a file that fail the schema check: the first line "  FAILED (n parts)" or none
failing_parts() {
    "$root/scripts/validate-docx.sh" --mce "$1" 2> /dev/null | sed -n 's/^  FAILED (\([0-9]*\) parts)$/\1/p; s/^  \(no such file\|not a zip file\)$/1/p' | head -n 1 || true
}

# the build, once
echo "building the test binary…" >&2
bin=$(cd "$root" && cargo test --locked --no-run --message-format=json 2> /dev/null \
    | python3 -c '
import json, sys
exe = None
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("target", {}).get("name") == "shale" and m.get("executable"):
        exe = m["executable"]
print(exe or "")
')
[ -n "$bin" ] && [ -x "$bin" ] || { echo "docx-corpus.sh: the test binary could not be built" >&2; exit 1; }
"$root/scripts/validate-docx.sh" --fetch

report=$keep/summary.tsv
printf 'name\twords back\tpages\tschema\terror\n' > "$report"
printf '%-40s %11s %9s %-14s %s\n' name "words back" pages schema error
bad=0
count=0
while IFS=$'\t' read -r name url hash _; do
    case $name in '' | '#'*) continue ;; esac
    [ -z "$only" ] || [ "$only" = "$name" ] || continue
    count=$((count + 1))
    error=
    problem=no
    if ! fetch "$name" "$url" "$hash"; then
        error="download: see above"
        printf '%-40s %11s %9s %-14s %s\n' "$name" - - - "$error"
        printf '%s\t-\t-\t-\t%s\n' "$name" "$error" >> "$report"
        bad=$((bad + 1))
        continue
    fi
    input=$cache/$name.docx
    out=$keep/$name.docx
    rm -f "$out"
    log=$work/run.log
    words=-
    if SHALE_ROUNDTRIP_BIN=$bin timeout 600 "$root/scripts/roundtrip-docx.sh" "$input" "$out" > "$log" 2>&1; then
        words=$(sed -n -e 's/^words that came back: [0-9]* of [0-9]* (\([0-9.]*\) %).*/\1%/p' -e 's/^words that came back: 0 of 0 (no words).*/no words/p' "$log")
    else
        error=$(grep -m1 -E '^(import|export) error:' "$log" || true)
        if [ -z "$error" ]; then
            message=$(grep -m1 -A1 'panicked at' "$log" | tail -n 1 | cut -c1-100 || true)
            [ -n "$message" ] || message=$(tail -n 1 "$log" | cut -c1-100 || true)
            error="crash: $message"
        fi
    fi
    page_text=-
    schema=-
    if [ -f "$out" ]; then
        before=$(page_count "$input")
        after=$(page_count "$out")
        [ $counter = none ] || page_text="$before→$after"
        mine=$(failing_parts "$out")
        theirs=$(failing_parts "$input")
        if [ -z "$mine" ]; then
            schema=ok
        else
            schema="FAIL $mine"
            [ -z "$theirs" ] || schema="$schema (orig $theirs)"
            [ -n "$theirs" ] && [ "$mine" -le "$theirs" ] || problem=yes
        fi
    fi
    [ -z "$error" ] || problem=yes
    [ $problem = no ] || bad=$((bad + 1))
    printf '%-40s %11s %9s %-14s %s\n' "$name" "$words" "$page_text" "$schema" "$error"
    printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$words" "$page_text" "$schema" "$error" >> "$report"
done < "$list"
[ $count -gt 0 ] || { echo "docx-corpus.sh: no file of the list matches" >&2; exit 2; }
echo "$count files, $bad with a problem; the exports and $report are in $keep (pages: $counter)" >&2
[ $bad -le 125 ] || bad=125
exit "$bad"

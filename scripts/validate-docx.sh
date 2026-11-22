#!/usr/bin/env bash
# Schema check of Word files (DEV-036, DOCX-010): every `word/*.xml` part of each file is formatted with
# `xmllint --format` (so an error names a line) and validated against Microsoft's WordprocessingML schema
# `wml-2010.xsd`, as pandoc's own tools/validate-docx.sh does.
#
#   scripts/validate-docx.sh [--fetch] [--mce] [file.docx]…
#
# The schemas are those of https://github.com/devoidfury/docx-validator at a pinned commit. Their license
# is unclear (NOASSERTION), so they are never committed: the 17 files that wml-2010.xsd needs (the list and
# the SHA-256 of each are below) are fetched into a cache outside the repository,
# ${XDG_CACHE_HOME:-~/.cache}/shale-dev/docx-schemas/<commit>/, checked one by one, and reused. The Rust tests
# (src/docx_export/schema_check.rs) read the same cache and skip when it is empty; `--fetch` fills it
# (and stops there when no file is named). Prints "ok" or the first errors of each failing part; the exit
# status is the number of files that failed (at most 125), 2 for a bad call, 1 when a tool or the schemas are missing.
# `--mce` leaves out what Markup Compatibility lets a reader ignore, which the schema does not know: the attribute
# `mc:Ignorable`, `mc:AlternateContent`, and attributes and elements of Microsoft's extension namespaces
# (`w14:paraId` and the like). Files made by Word have them all, so that is how a real file is checked.
set -euo pipefail

COMMIT=bfb31e310555a820f5f0813b0a2233ba5e9ca991
SHOWN=8

MANIFEST='4de4390a26e7e44e682ed98b39872cce2620f807f5b0de421567a43f74526388  ISO-IEC29500-4_2016/dml-chart.xsd
3fd0586f2637b98bb9886f0e0b67d89e1cc987c2d158cc7deb5f5b9890ced412  ISO-IEC29500-4_2016/dml-chartDrawing.xsd
809f77f658f71e10f5cfbf99e25c895a23a4e62c81210b59a2fe8c7fb24849d2  ISO-IEC29500-4_2016/dml-diagram.xsd
5cb76dabd8b97d1e9308a1700b90c20139be4d50792d21a7f09789f5cccd6026  ISO-IEC29500-4_2016/dml-lockedCanvas.xsd
6978ba7e889070b0c3cb5b546b23e5a6c3516134afc53b87a21f482ca33f3858  ISO-IEC29500-4_2016/dml-main.xsd
5d389d42befbebd91945d620242347caecd3367f9a3a7cf8d97949507ae1f53c  ISO-IEC29500-4_2016/dml-picture.xsd
2dbfd4719505bf39b568005994a433997cbeac59cebc9d4867d564c94e9ba03a  ISO-IEC29500-4_2016/dml-wordprocessingDrawing.xsd
48675c4f82f6b097434d4b7e313635b790257cca58fa90dd0e8f19c9affa18ed  ISO-IEC29500-4_2016/shared-commonSimpleTypes.xsd
0d103b99a4a8652f8871552a69d42d2a3760ac6a5e3ef02d979c4273257ff6a4  ISO-IEC29500-4_2016/shared-customXmlSchemaProperties.xsd
f812dffca0bb66db6e2ec2e01dd19258636b2a5a3a0bb949a2e7295665e0ce61  ISO-IEC29500-4_2016/shared-math.xsd
12264f3c03d738311cd9237d212f1c07479e70f0cbe1ae725d29b36539aef637  ISO-IEC29500-4_2016/shared-relationshipReference.xsd
cf90407251dff19633e9e397ea77a2457764132cf6b736301b48042ea1111354  ISO-IEC29500-4_2016/wml.xsd
70a3c67959f9ad2333016328716101bf8b476e682f29830cc9ee8da63dca7cc2  ISO-IEC29500-4_2016/xml.xsd
3a37e461ecf5a8670fdec34029703401f8728ab9c96ec1739a6ae58d55212413  mce/mc.xsd
ba105f6f2dac81ffdf3a2d541c4e1dee75889d123f7928b6e5a5dcc504c6f39a  microsoft/wml-2010.xsd
045ba07ba0191fd2c4f9b76718c6d026c4b472b8c89fdfa5939080e273b79fa6  microsoft/word12.xsd'

cache=${XDG_CACHE_HOME:-$HOME/.cache}/shale-dev/docx-schemas/$COMMIT
xsd=$cache/schemas/microsoft/wml-2010.xsd

need() {
    command -v "$1" > /dev/null 2>&1 || { echo "validate-docx.sh: $1 is not installed" >&2; exit 1; }
}

fetch() {
    if [ -f "$cache/.verified" ] && [ -f "$xsd" ]; then
        return 0
    fi
    need curl
    need sha256sum
    local tmp
    tmp=$(mktemp -d)
    local hash path
    while read -r hash path; do
        mkdir -p "$tmp/schemas/$(dirname "$path")"
        curl -fsSL "https://raw.githubusercontent.com/devoidfury/docx-validator/$COMMIT/schemas/$path" -o "$tmp/schemas/$path" \
            || { echo "validate-docx.sh: could not fetch $path" >&2; rm -rf "$tmp"; exit 1; }
        if [ "$(sha256sum "$tmp/schemas/$path" | cut -d' ' -f1)" != "$hash" ]; then
            echo "validate-docx.sh: $path does not have the pinned SHA-256" >&2
            rm -rf "$tmp"
            exit 1
        fi
    done <<< "$MANIFEST"
    touch "$tmp/.verified"
    rm -rf "$cache"
    mkdir -p "$(dirname "$cache")"
    mv "$tmp" "$cache"
    echo "schemas fetched to $cache" >&2
}

fetch_only=no
mce=no
files=()
for a in "$@"; do
    case $a in
        --fetch) fetch_only=yes ;;
        --mce) mce=yes ;;
        -h | --help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "validate-docx.sh: unknown option $a" >&2; exit 2 ;;
        *) files+=("$a") ;;
    esac
done
if [ ${#files[@]} -eq 0 ] && [ $fetch_only = no ]; then
    echo "usage: $0 [--fetch] [file.docx]…" >&2
    exit 2
fi

fetch
[ ${#files[@]} -eq 0 ] && exit 0
need xmllint
need unzip

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failed=0
for file in "${files[@]}"; do
    echo "*** Checking $file"
    if [ ! -f "$file" ]; then
        echo "  no such file"
        failed=$((failed + 1))
        continue
    fi
    rm -rf "${tmp:?}/part" && mkdir "$tmp/part"
    bad=0
    if ! parts=$(unzip -Z1 "$file" 2> /dev/null); then
        echo "  not a zip file"
        failed=$((failed + 1))
        continue
    fi
    while IFS= read -r name; do
        [[ $name =~ ^word/[^/]+\.xml$ ]] || continue
        part=$tmp/part/$(basename "$name")
        unzip -p "$file" "$name" > "$part"
        if ! xmllint --format "$part" > "$part.pretty" 2> "$part.err"; then
            echo "  $name: not well-formed XML"
            head -n "$SHOWN" "$part.err" | sed 's/^/    /'
            bad=$((bad + 1))
            continue
        fi
        ok=yes
        xmllint --noout --nonet --schema "$xsd" "$part.pretty" > /dev/null 2> "$part.err" || ok=no
        grep -v -E ' validates$| fails to validate$' "$part.err" > "$part.errors" || true
        if [ $ok = no ] && [ $mce = yes ]; then
            grep -v -E "(attribute|Element) '\{(http://schemas.openxmlformats.org/markup-compatibility/2006|http://schemas.microsoft.com/office/)" "$part.errors" > "$part.kept" || true
            mv "$part.kept" "$part.errors"
            [ -s "$part.errors" ] || ok=yes
        fi
        if [ $ok = no ]; then
            echo "  $name:"
            sed -e "s|$tmp/part/||" -e 's|{http://schemas.openxmlformats.org/wordprocessingml/2006/main}|w:|g' "$part.errors" | head -n "$SHOWN" | sed 's/^/    /'
            bad=$((bad + 1))
        fi
    done <<< "$parts"
    if [ $bad -eq 0 ]; then
        echo "  ok"
    else
        echo "  FAILED ($bad parts)"
        failed=$((failed + 1))
    fi
done
[ $failed -le 125 ] || failed=125
exit "$failed"

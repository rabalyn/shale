#!/bin/sh
# Copy the user guide vault (docs/userguide, DEV-020) into the folder given, without what Shale
# writes into a vault it opens: .shale/state.json, .shale/tag-words.json and .trash/. The same
# three names are `NOT_COPIED` in src/userguide.rs (a test checks that both lists agree).
#   scripts/userguide-dist.sh dist/shale/docs/userguide  in the archive (FEATURES.md links into it)
#   scripts/userguide-dist.sh dist/zip/userguide         what the zip of the release holds
set -eu
dest=${1:?usage: userguide-dist.sh <folder to copy the guide into>}
src=$(dirname "$0")/../docs/userguide
test -f "$src/Start here.md" || { echo "userguide-dist.sh: no guide at $src" >&2; exit 1; }
mkdir -p "$dest"
tar -C "$src" --exclude='.shale/state.json' --exclude='.shale/tag-words.json' --exclude='.trash' -cf - . | tar -C "$dest" -xf -

#!/bin/sh
# Writes the seeded calendars into the volume at /data (once: nothing happens if they exist).
# The .ics files in data/ hold day markers such as @+3@ (three days from today, UTC) and @-14@;
# they are replaced by dates (YYYYMMDD) here, so the events are always inside the window Shale
# loads (4 months back to 13 months ahead).
set -eu
root=/data/collections
if [ -d "$root/collection-root/shale" ]; then
  echo "already seeded"
  exit 0
fi
mkdir -p "$root"
cp -r /seed/data/. "$root/"
now=$(date +%s)
find "$root" -name '*.ics' | while read -r f; do
  for n in $(grep -o '@[+-][0-9]*@' "$f" | sort -u | tr -d '@'); do
    day=$(date -u -d "@$((now + n * 86400))" +%Y%m%d)
    pat=$(printf '%s' "$n" | sed 's/^+/[+]/')
    sed -i "s/@${pat}@/$day/g" "$f"
  done
done
echo "seeded"

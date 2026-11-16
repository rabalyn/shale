#!/bin/sh
# Makes the small sound files that the media tests use (MEDIA-011, DEV-004): a one second tone
# of 440 Hz in each format Shale plays, each also with a cover (red left, blue right, 64 x 48)
# in the kind of tag the format has. The files are committed; run this only to change them.
#
#   sh scripts/make-media-fixtures.sh
#
# Needs ffmpeg (with libmp3lame, libvorbis, libopus) and python3 (for the Vorbis picture tag,
# which ffmpeg cannot write itself). The files stay under 30 KB each.
set -eu
cd "$(dirname "$0")/.." || exit 1
out=src/testdata/media
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }

# the cover: two halves, red and blue
ff -f lavfi -i color=c=red:s=32x48 -f lavfi -i color=c=blue:s=32x48 -filter_complex hstack -frames:v 1 "$tmp/cover.png"
tone="-f lavfi -i sine=frequency=440:duration=1:sample_rate=44100"
cover="-i $tmp/cover.png"
attach="-map 0:a -map 1:v -c:v copy -disposition:v attached_pic"

# mp3: ID3v2 APIC
ff $tone -c:a libmp3lame -b:a 48k -ac 1 "$out/tone.mp3"
ff $tone $cover -map 0:a -map 1:v -c:a libmp3lame -b:a 48k -ac 1 -c:v copy -id3v2_version 3 -metadata:s:v comment="Cover (front)" "$out/tone-cover.mp3"
# ogg, oga (Vorbis) and opus: Vorbis comment METADATA_BLOCK_PICTURE (a FLAC picture block, base64)
picture=$(python3 - "$tmp/cover.png" <<'PY'
import base64, struct, sys
png = open(sys.argv[1], "rb").read()
mime = b"image/png"
block = struct.pack(">II", 3, len(mime)) + mime + struct.pack(">I", 0) + struct.pack(">IIII", 64, 48, 24, 0) + struct.pack(">I", len(png)) + png
print(base64.b64encode(block).decode())
PY
)
ff $tone -c:a libvorbis -q:a 0 -ac 1 "$out/tone.ogg"
ff $tone -c:a libvorbis -q:a 0 -ac 1 "$out/tone.oga"
ff $tone -c:a libvorbis -q:a 0 -ac 1 -metadata "METADATA_BLOCK_PICTURE=$picture" "$out/tone-cover.ogg"
cp "$out/tone-cover.ogg" "$out/tone-cover.oga"
ff $tone -c:a libopus -b:a 24k -ac 1 "$out/tone.opus"
ff $tone -c:a libopus -b:a 24k -ac 1 -metadata "METADATA_BLOCK_PICTURE=$picture" "$out/tone-cover.opus"
# flac: PICTURE block
ff $tone -c:a flac -ac 1 -sample_fmt s16 "$out/tone.flac"
ff $tone $cover $attach -c:a flac -ac 1 -sample_fmt s16 "$out/tone-cover.flac"
# m4a: MP4 covr atom
ff $tone -c:a aac -b:a 48k -ac 1 "$out/tone.m4a"
ff $tone $cover $attach -c:a aac -b:a 48k -ac 1 "$out/tone-cover.m4a"
# aac (ADTS) and wav have no cover
ff $tone -c:a aac -b:a 48k -ac 1 -f adts "$out/tone.aac"
ff -f lavfi -i sine=frequency=440:duration=1:sample_rate=8000 -c:a pcm_s16le -ac 1 "$out/tone.wav"
ls -l "$out"

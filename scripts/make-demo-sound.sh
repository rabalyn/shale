#!/bin/sh
# Makes the sound of the media screenshot (docs/screenshots/fixtures/media-vault, DEV-007); the file is
# committed. Run it from the repository root: sh scripts/make-demo-sound.sh [out.mp3]
# Generates "Sea with birds": ~14 s of synthetic surf with distant bird calls, and a cover picture.
# Everything is computed (noise + tones), no recordings. Needs ffmpeg.
set -eu
out=${1:-docs/screenshots/fixtures/media-vault/z_media/sea-with-birds.mp3}
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
D=14
# Waves: brown noise for the body, pink noise for the foam, both swelling with a slow, uneven rhythm.
ffmpeg -v error -y -f lavfi -i "anoisesrc=d=$D:c=brown:r=44100:a=0.9:s=7" \
  -af "lowpass=f=700,volume='0.12+0.88*pow(0.5+0.5*sin(2*PI*t/6.5)*sin(2*PI*t/6.5+0.6),1.5)':eval=frame" "$tmp/body.wav"
ffmpeg -v error -y -f lavfi -i "anoisesrc=d=$D:c=pink:r=44100:a=0.5:s=11" \
  -af "highpass=f=1800,lowpass=f=9000,volume='0.05+0.6*pow(max(0,sin(2*PI*(t-0.9)/6.5)),6)':eval=frame" "$tmp/foam.wav"
# Birds: short warbling chirps (a tone gliding down with a fast vibrato), in small groups, quiet.
chirp() { # start, base frequency
  echo "exp(-pow((t-$1-0.06)/0.025,2))*sin(2*PI*($2*(t-$1)-900*pow(t-$1,2))+3*sin(2*PI*28*(t-$1)))"; }
B=""
for s in "1.2 3400" "1.42 3600" "1.66 3300" "4.8 2900" "4.95 3050" "7.9 3800" "8.1 3700" "8.35 3900" "8.6 3600" "11.3 3100" "11.5 3250" "12.6 4000"; do
  set -- $s; c=$(chirp "$1" "$2"); B="${B:+$B+}$c"; done
ffmpeg -v error -y -f lavfi -i "aevalsrc='0.22*($B)':s=44100:d=$D" -af "aecho=0.6:0.5:120:0.25" "$tmp/birds.wav"
# Mix: waves left/right slightly apart, birds a little to the right; fade in and out; never above full scale.
ffmpeg -v error -y -i "$tmp/body.wav" -i "$tmp/foam.wav" -i "$tmp/birds.wav" -filter_complex \
 "[0]asplit[a][b];[b]adelay=40[bd];[a][bd]join=inputs=2:channel_layout=stereo[w];\
  [1]pan=stereo|c0=0.8*c0|c1=c0[f];[2]pan=stereo|c0=0.5*c0|c1=c0[bi];\
  [w][f][bi]amix=inputs=3:weights=1 0.8 1:normalize=0,afade=t=in:d=1.5,afade=t=out:st=$((D-2)):d=2,alimiter=limit=0.89[out]" \
 -map "[out]" "$tmp/mix.wav"
# Cover: sky, sun and sea with soft wave bands, 400x400.
ffmpeg -v error -y -f lavfi -i "color=c=black:s=400x400:d=1" -vf "geq=\
r='clip(if(lt(Y,230), 110+110*Y/230 + 140*exp(-(pow(X-270,2)+pow(Y-150,2))/700), 20+25*(1+sin(Y/5+X/45))*(Y-200)/200),0,255)':\
g='clip(if(lt(Y,230), 150+70*Y/230 + 110*exp(-(pow(X-270,2)+pow(Y-150,2))/700), 70+30*(1+sin(Y/5+X/45))),0,255)':\
b='clip(if(lt(Y,230), 225+25*Y/230 - 120*exp(-(pow(X-270,2)+pow(Y-150,2))/700), 120+40*(1+sin(Y/5+X/45))),0,255)'" -frames:v 1 "$tmp/cover.png"
ffmpeg -v error -y -i "$tmp/mix.wav" -i "$tmp/cover.png" -map 0 -map 1 -c:a libmp3lame -b:a 64k \
  -c:v png -disposition:v attached_pic -id3v2_version 3 \
  -metadata title="Sea with birds" -metadata artist="Shale demo (generated)" \
  -metadata:s:v title="Album cover" -metadata:s:v comment="Cover (front)" "$out"

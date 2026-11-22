Play audio and video files inside your notes. Back to [[Start here]]; embeds in general are in
[[Notes and links]].

# Sounds

Embed a sound like a picture, with `![[file.mp3]]`. Here is one:

![[sea-with-birds.mp3]]

![[sound-player.png]]

- **Formats.** mp3, ogg, oga, flac, m4a, aac and wav play. opus files are recognised and get a
  player, but cannot be played yet.
- **Where.** The player shows in the editor, the markdown preview and the [[Reading view]].
  A link without the `!` stays a plain link.
- **The card.** It shows the cover from the file's tags, or a tile with the file name.
- **Play and pause.** The round button over the card. It is half transparent and fully opaque
  when the mouse is over it; how opaque it is (60 % by default) is set in Settings →
  Appearance (`media_play_button_opacity`). In the reading view, after you have clicked the card, Space does the same. In the editor the player is worked with the mouse alone: a click on it does not take the keyboard from the text, so your cursor stays where it was and Space types a space.
- **Progress.** The bar shows the time. Click it to jump there; drag it to choose a place and
  let go to seek.
- **Volume.** The speaker button mutes, the slider sets the volume. It is one volume for all
  players (`media_volume`, 80 % by default). See [[Settings reference]].
- **One at a time.** Starting a player pauses the one that was playing. A sound keeps playing
  while you scroll or switch between the editor, the preview and the reading view, and stops when
  the note's tab is closed or another vault is opened.
- **Never by itself.** Nothing ever starts playing until you press the button or Space.
- **No sound output.** On a computer without a sound device the player says "No sound output" and
  a sound file does not play. A video still plays its pictures, without sound, and says "No sound
  output: playing without sound".

# Videos

Embed a video with `![[clip.mp4]]` (mp4, m4v, mov, webm and mkv). It has the same player as a
sound, with a 16:9 picture (the first frame until you play) and the controls below it. While it
plays, the picture shows the video and the sound goes through the same output as sound files.
A click on the progress bar restarts the video at that place. A click before the video's length
is known (the total time still shows `–:––`) is kept on the bar and the video jumps there as soon
as the length arrives. Frames are shown at most 1280 × 720 pixels and 30 per second.

![[video-resize.webm]]

*(That clip is a webm file, played by this player. It shows how to resize a video; see
[[Editor]].)*

A video can be resized like a picture, see "Resizing pictures and videos" in [[Editor]].

## ffmpeg

Videos are decoded by `ffmpeg` and `ffprobe` on your computer. They are optional: install them
with `apt install ffmpeg` (Linux), `brew install ffmpeg` (macOS) or `winget install ffmpeg`
(Windows), or get them from https://ffmpeg.org/download.html. Shale finds them by itself; open
the note again after installing.

Without them the video shows a tile with a play mark, and a click opens the file in your
system's video player. Settings → Appearance says where ffmpeg was found, or that it
is missing and where to get it.

# Media from other computers

When a vault is synced with [[Git sync]] and its media are stored with Git LFS, a sound or
video that came from another computer first arrives as a small pointer file. Its player shows
"Not downloaded" with the size and a **Download** link. Pressing play downloads the file first
and then plays it. A picture embed does the same. This only works once the other computer has
synced, so that the file is on the server.

To download every media file after each sync instead, tick "Download media files after every
sync" in Settings → Sync (`sync.lfs_download`: `on_demand` or `always`, see
[[Settings reference]] and [[Vault settings]]). If a download fails, the player says why and
**Download** asks again.

#userguide #media

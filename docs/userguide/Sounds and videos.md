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

Embed a video with `![[clip.mp4]]` (mp4, m4v, mov, webm, mkv, avi and ogv). It has the same player as a
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

# Files outside the vault

You do not have to copy big media files into the vault. A note can link a picture, PDF, sound or
video where it is. There are two forms, and both show like a file of the vault, with the same
players, in the editor, the preview, the [[Reading view]] and the [[PDF]] export:

- **A library link**, written `![[@Videos/2024/a.mp4]]` (or `[[@Videos/doc.pdf]]` as a plain
  link). `Videos` is a *library folder*: every computer says in its own settings (`media_libraries`
  in the settings file, see [[Settings reference]]) which folder it stands for, so the same note
  works on each of your computers although the folders differ. A file of the vault at that path wins,
  and the rest of the path must stay inside the library folder (no `..`).
- **A file link**, written `![](<file:///home/ada/Videos/a.mp4>)`, also with `%20` for a space and
  as `file:///C:/Users/ada/a.mp4` on Windows. It is the form Obsidian writes, and it names one file on
  this computer only.

If the file is not there the note says "Not found:" with its path; if the library is not set up on
this computer it says "Library “Videos” is not set up on this device". Nothing is created. A click
on such a link opens the file in your system's program, and nothing else than pictures, PDFs,
sounds and videos is ever opened this way. These links are not red, and they are not in the graph.

Such files often live on a NAS, a network share or a USB disk. Shale never waits for the disk: it
looks at the file in the background, and until it has an answer the embed says "Loading …" in a quiet
strip (a picture is decoded in the background too). It asks again every few seconds, so a file that
appears or goes is noticed without anything else changing, and the window keeps working while a
drive does not answer.

Files outside the vault are not part of it: they are not synced with [[Git sync]] and not in
the [[File history]], and a backup of the vault does not hold them. Shale says so where it matters:
the caption of such a picture or player has the tooltip "Outside the vault: not synced", the reading
view prints it under the player, and Settings → Sync tells how many files the vault links from outside.

## Copy into the vault later

A link to a file outside the vault can become an ordinary file of the vault at any time:

- **One file**: below an outside picture, PDF, sound or video in the editor (and the markdown preview)
  the caption has **Copy into the vault** at its right end; in the [[Reading view]] it is a button
  beside **Open** on a picture or PDF and a line under a player. It is shown only when the file is
  there. A press on it does only that: the cursor stays where it was.
- **The whole note**: ⋯ → **Copy outside media into the vault** copies every outside file the note
  links, whose file is there.

The files are copied to `z_media/<year>/<month>/` like any other file you add (a file that is linked
twice, or in both forms, is copied once), and the links in the note then point at the copies: an
embed keeps its width (`![[…|300]]`), its words and its form. In the editor this is one step for
Ctrl+Z, which brings the outside links back (the copies stay in `z_media`). The copying runs in the
background; the message tells how many files went into the vault and names those that were left
because their file is not there or could not be copied. The original files are never changed.

## Checking what the vault links

**Settings → Vault → Check names for Windows and macOS…** also lists the links to media outside the
vault ([[Vault settings]]): the libraries that are not set up on this computer or whose folder is
not there, and for each note the links whose file is not found, whose library is missing, and the file
links (`file://`), which work on this computer only. **Data usage…** ([[Data usage and caches]])
shows how many outside files the vault links and how big they are, and how many are missing.

## Linking instead of copying

**Settings → Import → Files from outside the vault** says what happens to a picture, PDF, sound or
video that is not in the vault when you pick it with `[!`, paste it or import it:

- **Ask** (the start value): a small question each time, "Copy into the vault" or "Link where they
  are". In the Import dialog every such file has the choice itself, starting at Copy.
- **Copy into the vault**: a copy goes to `z_media/` as always, and is synced with the vault.
- **Link where they are**: nothing is copied; the note gets a link to the file.

Shale never links a file by itself because it lies in a library folder: it follows the setting
every time. Other files (notes, documents, anything that is not a picture, PDF, sound or video) are
always copied, and so are the pictures of a post or of a Word file, which are not files from outside.

What a link looks like: a file under one of your **library folders** becomes `![[@Videos/2024/a.mp4]]`
(`[[@Videos/doc.pdf]]` for a PDF), any other file `![](<file:///home/ada/Videos/a.mp4>)` (`[doc.pdf](<file:///…>)` for a
PDF); the folder with the most parts wins when library folders lie inside each other. Names that
`[[ ]]` cannot hold (a `#`, `|`, `^`, a bracket in a folder or file name) get the second form. Obsidian
reads the second form as well.

The library folders are in the same place in Settings: a name and a folder per line, **Add library**
at the end and **Remove** on each line. The name has no `/` and none of `# | ^ [ ]`, works on Windows and is
not the name of another library; Shale says what is wrong in red, and also when the folder is not
there. A library only exists on this computer: set up the same name on your other computers, with
their own folders, and the same note shows the same file on each.

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

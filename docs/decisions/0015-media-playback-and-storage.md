# ADR-0015: Media playback in notes, and media files in Git LFS

- **Status**: Accepted (owner decision, 2026-11-16)
- **Date**: 2026-11-16
- **Specs**: MEDIA-001…MEDIA-011, SYNC-016…SYNC-022, DEV-018
- **Issues**: raba/shale#178 (audio), raba/shale#179 (video), raba/shale#180 (LFS setup),
  raba/shale#181 (LFS download on demand), raba/shale#182 (user guide vault)
- **Amends**: [ADR-0008](0008-git-sync.md) ("Large binary attachments go into git history")

## Context

Notes link audio and video files (`z_media/…`, imports, raba/shale#175), but Shale shows only
pictures, drawings and PDFs inline; everything else is "can't be shown here yet". The owner wants
a small player inside the note: artwork (embedded cover or first video frame), a play/pause
button over it, a seek bar that jumps without clicks, and a volume slider.

Media files are large and binary. ADR-0008 commits everything into git, so every version of every
video stays in the history and every clone downloads all of it.

## Decision

### Playback

1. **Audio in pure Rust, with an engine of our own.** `symphonia` decodes (mp3, Vorbis/Opus in
   ogg, FLAC, AAC/ALAC in MP4, WAV) and reads the tags, `cpal` plays, `rubato` resamples when the
   device rate differs. A decoder thread fills a ring buffer that the cpal callback reads. No
   `rodio`: we need exact control over fades and gain.
2. **No clicks, no clipping.** Seek, start and pause ramp the gain over about 5 ms; dragging the
   seek bar only previews the position and seeks once on release. The volume is a smoothed gain
   in 0…1 and is never above 1, so the output never goes beyond full scale.
3. **Video through the system `ffmpeg`**, run like git (SYNC-001): an argument list, no shell,
   stdin closed. The first frame is the thumbnail. Playback pipes raw RGBA frames into a texture
   and the sound as f32 into the audio engine; the audio clock leads. A seek restarts ffmpeg at
   `-ss <t>`. **Without ffmpeg** the video shows a placeholder, and a click opens the file with
   the system's default program for its type (`opener`).
4. **One player widget** for both, in the reading view, the inline editor embeds (EDIT-006) and
   the markdown preview (EDIT-016). One playback at a time. Settings in the global config:
   `media_volume`, `media_play_button_opacity` (default 0.6), `media_autoplay` (default off).
   *Note, 2026-11-16, owner: no autoplay, ever; the setting `media_autoplay` was dropped (MEDIA-007).*
5. Code lives in a feature folder `src/media/` (DEV-008); `ShaleApp` gets one `MediaPlayer`
   sub-state.

### Storage: Git LFS, set up by the sync

6. Media files are stored with **Git LFS**. Shale sets it up in the vault's repository, using the
   same `git` the sync runs. The server side is the remote's LFS store; Forgejo has one built
   in, and its `[lfs]` storage can be an S3/MinIO bucket, so the files can live somewhere else
   without Shale knowing a second protocol.
7. **Download on demand.** The repository is set up with `--skip-smudge`, so other machines get
   small pointer files; a file is downloaded when it is played or opened.

#### Git LFS setup procedure

**A. Check.** `git lfs version`. Missing → the sync settings say "Git LFS is not installed"
with where to get it (BUILD-018); nothing else changes, and step E guards the repository.

**B. Set up a repository** (the button "Store media with Git LFS" in the sync settings; done
on its own when Shale's connect creates a new repository, SYNC-004):

1. Refuse while a merge or rebase runs (SYNC-005), and hold the sync's tree lock.
2. `git lfs install --local --skip-smudge`: hooks and filter config in `.git/` only. The
   user's global git config is never touched.
3. Write Shale's block into `.gitattributes` (created if missing), between the lines
   `# shale-media-begin` and `# shale-media-end`, one line per pattern:
   `*.<ext> filter=lfs diff=lfs merge=lfs -text` for mp3 ogg oga opus flac m4a aac wav mp4 m4v
   mov webm mkv avi ogv, upper case too (avi and ogv were added with #206). Lines outside the block are the user's and are kept.
4. Existing media: `git add --renormalize -- <patterns>`, then one commit
   `<prefix>Store media files with Git LFS<suffix>`. History is **not** rewritten: the old
   versions stay in it. Removing them is `git lfs migrate import` plus a force push, which
   Shale never does (SYNC-012); the user guide explains it as a manual step.
5. The next sync pushes as usual (C).

**C. Every sync** (additions to ADR-0008 §4):

- Before committing: step E.
- Before `git push`: `git lfs push origin <branch>` (explicit, so it works even with hooks
  turned off).
- After a merge brings in `.gitattributes` with LFS lines (another machine set it up): set up
  this repository as in B.2 when git-lfs is installed; otherwise E applies.
- Conflict copies (SYNC-008) of LFS files are LFS files too.

**D. Download.** A pointer file (starts with `version https://git-lfs.github.com/spec/v1`) is
shown as "not downloaded" with its size; play or "Download" runs
`git lfs pull --include=<path> --exclude=""` in a background job that holds the tree lock.
Vault setting `sync.lfs_download`: `on_demand` (default) or `always` (`git lfs pull` after every
sync).

**E. Guard.** If `.gitattributes` sends paths to LFS but this repository has no LFS filter set
up and git-lfs is missing, the sync does not commit and says why. A media file must never land
in plain history only because one machine lacks git-lfs.

**Server.** Forgejo: LFS is on by default (`[server] LFS_START_SERVER = true`); SSH and HTTPS
remotes both work with the user's git credentials. The guide shows the Forgejo `[lfs]` storage
settings for S3/MinIO.

**Tests.** A bare `file://` remote (git-lfs transfers to local remotes by itself); tests skip
with a note when git-lfs is missing (BUILD-018); CI installs `git-lfs`. A Forgejo test stack
(DEV-005) can follow.

### Documentation

8. The user guide becomes a Shale vault (DEV-018, raba/shale#182): notes, screenshots and short
   videos played by this player. Every change to a feature or setting updates it (DEV-006).

## Consequences

- Linux builds need `libasound2-dev` (cpal); CI and the build notes install it.
- Video needs ffmpeg on the machine; without it videos still open in the system player.
- A vault with LFS needs git-lfs on every machine that commits media; the guard makes a missing
  install visible instead of silently filling the history.
- Media already in history stays there unless the user migrates by hand.

## Alternatives considered

- *rodio*: quicker start, but no control over seek fades. Rejected (owner: own engine is fine).
- *ffmpeg or GStreamer linked into the binary*: in-process and hardware decoding, but heavy native
  builds on macOS and Windows. *openh264*: H.264 only. Rejected for the system ffmpeg.
- *Shale's own media store* (manifest in git, files on WebDAV/S3 by hash): most flexible, but a
  second LFS to build and maintain. Kept as an option if a non-git backend is ever needed.
- *git-annex*: powerful, but awkward on Windows and too much to ask of users.

---
features:
  - "File history"
---
Shale keeps earlier versions of your notes and drawings, so that you can go back. Back to [[Start here]]. It works with or without a git server ([[Git sync]]).

# What is kept

- A version of a note or drawing is kept when you save it (at most one per note every 10 minutes while
  you type), when you stop typing for a minute, when you close it and when the window closes.
  Only saved text is kept, never what is still being typed. Two saves with the same text make one version.
- Notes and drawings are kept; sounds, videos and pictures are not (they are big, and they do not change).
  Temporary files of cloud folders and network shares are never kept.
- Nothing is deleted from the history: Shale never thins it out or rewrites it.

# Where it is kept

- A vault that syncs with a git server (or is a git repository already) keeps its versions in that
  repository, and the sync sends them to the server together with everything else.
- Any other vault keeps its history on this computer, in the folder `shale/history/<vault id>` of your data
  folder, **outside the vault**. The vault holds only the small file `.shale/history-id` that names its
  history; it travels with the vault, so the same vault on two computers has the same name for its
  history and each computer keeps its own. Moving or renaming the vault folder keeps the history; a copy of
  a vault gets a history of its own.
- A new history starts with a version of every note and drawing as they are. Settings → Sync shows where it is
  kept. [[Data usage and caches]] shows how big it is.

# Go back to a version

- Open the note's **⋯** menu and choose **Versions of this note…** (a drawing has the same). Each row is a version with
  its time, what caused it (saved, idle, closed, renamed, restored, synced…), the computer and the lines added and
  removed.
- **Restore** puts that version into the note as a new change: the text you have now is kept as a version first, the
  note shows the old text, and one undo (vim-style `u`) brings back what you had. Earlier versions stay as they were.
- **Copy text** copies that version to the clipboard. **Read-only** shows it in the window without changing the note.
- A note that was renamed or moved keeps its versions together; they are listed under the new name.

# Restore a deleted note

Settings → Sync → **Restore a deleted note…** (also the **Deleted notes** tab of the versions list) lists notes
that were deleted or moved to the trash, newest first. **Restore** writes the last text back at its place, or as
`Name (restored).md` when something is there already.

# Settings

Settings → Sync, **File history** ([[Settings reference]]): switch it off, or change how often versions are kept
(`history.enabled`, `history.coalesce_minutes`, `history.idle_seconds`). Without git the history is off and the
page says so ([[Git sync]] explains how Shale can install one).

#userguide #git

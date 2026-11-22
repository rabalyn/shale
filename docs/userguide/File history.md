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

# See the history of a note

- With one note open in a wide window, the history is a slim column in the empty margin at the right of the
  note. In a narrower window, with several notes side by side or for a drawing, it is the **History** panel of
  the right sidebar (the clock icon, next to the outline and the calendar). The **×** of the column hides it;
  **Show it here** in the panel, or **History of this note** in the note's **⋯** menu, shows it again, and Shale
  remembers your choice.

![[history-margin.png]]

![[history-sidebar.png]]

- The list is grouped by day (Today, Yesterday, then the date). Each version says when ("5 min ago", then the time), where
  it came from in words (edited here, synced from your laptop, restored version of …, merged after a conflict, renamed from …)
  and how much changed: five small ticks, green for added and red for removed lines, with the counts beside them
  (`+12 −3`). Hold the pointer on a version for the exact time and "12 lines added, 3 removed". A file that is not text just says
  "changed".
- Without history (switched off, or no git) the panel says why and has a button to Settings → Sync.
- The history holds notes and drawings only. Media files that a note links from outside the vault ([[Sounds and videos]]) are never part of it.

![[file-history.webm]]

# Go back to a version

- Click a version: **Changes since 11:05** shows your note with the changes marked. Words that were added are
  highlighted and underlined, words that were deleted are struck through, and a small mark at the left of every changed
  paragraph, heading, list item or table row says **+** (added), **−** (removed) or **~** (changed), so you never need
  the colours to tell them apart. Parts that did not change are folded ("… 34 unchanged paragraphs"); click to open them.

![[history-changes.png]]

- The **slider** moves the older end over all versions, and the list at the right (**to**) picks the newer end, so you can
  compare any two versions. **Next change** and **Previous change** jump from change to change.
- **Restore this version** puts that version into the note as a new change: the text you have now is kept as a version first, the
  note shows the old text, and one undo (vim-style `u`) brings back what you had. Earlier versions stay as they were.
- **Copy text** copies that version to the clipboard. **Read-only** shows it in the window without changing the note.
- For experts: **Source, side by side** shows the Markdown, old on the left and new on the right, with line numbers
  and the changed words marked, and **Copy as patch** copies the change as text for other tools.

![[history-source.png]]

- A drawing shows the drawing before and after. A note that was renamed or moved keeps its versions together; they are listed
  under the new name.

# Restore a deleted note

Settings → Sync → **Restore a deleted note…** (also at the bottom of the history panel) lists notes
that were deleted or moved to the trash, newest first. **Restore** writes the last text back at its place, or as
`Name (restored).md` when something is there already.

# Settings

Settings → Sync, **File history** ([[Settings reference]]): switch it off, or change how often versions are kept
(`history.enabled`, `history.coalesce_minutes`, `history.idle_seconds`, and `history.show_panel` for the column beside the note). Without git the history is off and the
page says so ([[Git sync]] explains how Shale can install one).

#userguide #git

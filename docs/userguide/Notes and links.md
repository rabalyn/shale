---
features:
  - "Notes and links"
---
# Notes and links

A vault is a folder of markdown files. Notes link to each other with `[[wiki links]]`.
Back to [[Start here]].

![[editor.png]]

## Links

- `[[Note]]` is **blue** when the note exists and **red** when it does not. Click a blue link to
  open the note; click a red one to create it (next to the current note if you set
  "Create notes from red links next to the current note").
- `[[Note|shown text]]` shows another text, `[[Note#Heading]]` goes to a heading.
- A link by name (`[[Editor]]`) finds the note wherever it is; a link by path
  (`[[Folder/Note]]`, `./Note`, `../Note`) is exact.
- Web links `[text](https://…)` open in your browser.
- Settings → General → Links: follow links only with Ctrl+click, create notes next to the
  current note, update links when renaming (see [[Settings reference]]).

## Suggestions while you type

![[link-suggestions.png]]

![[link-completion.webm]]

- Type `[[` to get matching notes; the last entry creates a new note. Esc closes the list.
- Type `[!` to pick a file: `/` starts at the vault, `~/` at your home folder, and Tab
  completes like a shell. Files from outside the vault are copied to `z_media/<year>/<month>/`.
- Type `[[p ` and a name to link a person ([[Tags and people]]).
- In these lists ↓ / ↑ or Ctrl+J / Ctrl+K move the highlight; Enter, Tab or a click picks the entry.

## Embeds

Pictures `![[photo.png]]`, drawings `![[Sketch.excalidraw]]` and PDFs `![[Paper.pdf]]` are shown
inside the editor; the link text comes back while the cursor is in the link. The pictures in
this guide are embedded exactly like this. Switch it off in Settings → General → Appearance
("Show embedded images inside the editor").

**Captions.** Every embed that is shown has its link as a small caption below it: the file's
link (`z_media/photo.png`), or the words you give it after a `|`: `![[photo.png|Sea at dusk]]`
(and `![[photo.png|Sea at dusk|300]]` with a width). Click the caption to open the file. The
[[PDF]] export draws the same thumbnails with the same captions.

**Resizing.** Move the mouse over a picture or a video in the editor: its edges and corners
become handles, and a drag changes the size (the proportions stay). The width is written into
the link when you let go: `![[photo.png|300]]` (points; this is also what Obsidian writes), and
the reading view shows the embed at that width. Two round buttons sit in the corner of the
picture, looking like the play button of the sound player: **100%** sets the natural size, and the
fullscreen button shows the picture (or the video, with its controls) over the whole window; Esc,
the cross or a click beside it closes it.

## Moving and renaming

![[tree-drag.png]]

Drag notes, drawings, files and folders in the file list into another folder. Rename a note from
its menu. In both cases links in other notes (and in drawings) are updated, so nothing breaks.

## The note menu

![[more-menu.png]]

Export to PDF ([[PDF]]), insert a new drawing, rename, or move to the trash. The trash keeps
the original folder (`.trash/<date>/<path>`), and "Restore last deleted" in the file list's
context menu brings the last item back.

## Go back and forward

Mouse side buttons or Alt+← / Alt+→. Sort the file list by name, date modified or date
created (the sort icon above the list).

Next: [[Editor]], [[Tabs]], [[Search and pickers]].

#userguide #notes

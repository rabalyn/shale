---
features:
  - "Notes and links"
---
A vault is a folder of markdown files. Notes link to each other with `[[wiki links]]`.
Back to [[Start here]].

![[editor.png]]

# Links

- `[[Note]]` is **blue** when the note exists and **red** when it does not. Click a blue link to
  open the note; click a red one to create it (next to the current note if you set
  "Create notes from red links next to the current note").
- `[[Note|shown text]]` shows another text, `[[Note#Heading]]` goes to a heading: the note opens with
  that heading at the top of the view, in the mode the tab is in (the cursor on the heading in the
  editor, the reading view scrolled to it in the reading view). `[[#Heading]]` is a heading of this
  note, and `[text](Note.md#Heading)` works too. The heading is found ignoring case, spaces and
  formatting (`#plan v2` finds `## **Plan** v2`); if there is none, the note opens at its top and a
  message says so.
- A link by name (`[[Editor]]`) finds the note wherever it is; a link by path
  (`[[Folder/Note]]`, `./Note`, `../Note`) is exact.
- Web links `[text](https://…)` open in your browser.
- Settings → Editor → Links: follow links only with Ctrl+click, create notes next to the
  current note, update links when renaming (see [[Settings reference]]).

# Suggestions while you type

![[link-suggestions.png]]

![[link-completion.webm]]

- Type `[[` to get matching notes; the last entry creates a new note. Esc closes the list. With
  "Date prefix for new notes" on, the new note is named `YYMMDD_<name>` and the link you followed is
  rewritten to `[[261117_Plan|Plan]]` so it still reaches the note.
- Type `[!` to pick a file: `/` starts at the vault, `~/` at your home folder, and Tab
  completes like a shell. Files from outside the vault are copied to `z_media/<year>/<month>/`.
- Type `[[p ` and a name to link a person ([[Tags and people]]).
- After `[[Note#` (or `[[#` for this note) the list shows that note's headings, indented by level
  and with their numbers, and typing narrows it; picking one inserts the heading's text.
- In these lists ↓ / ↑ or Ctrl+J / Ctrl+K move the highlight; Enter, Tab or a click picks the entry.

# Numbered headings

The headings of a note can show numbers before their text: `#` is 1, a `##` under it 1.1, the next
`##` 1.2, the next `#` 2. The note's title is the title line above the text ([[Editor]]), not a heading, so a `#` is
numbered like the others. The numbers
are only drawn, in a muted colour, never written into the file; you see them in the editor, the
Markdown preview, the [[Reading view]], the outline ([[Graph and backlinks]]), the link preview and
the [[PDF]]. Links stay `[[Note#Heading]]`, with the heading's text. Settings → Editor → "Number
headings" (on by default) switches it for all notes; put `numbered-headings: true` or `false` (or
`yes` / `no`) in a note's front matter to decide for that note alone (see [[Settings reference]]).

# Embeds

Pictures `![[photo.png]]`, drawings `![[Sketch.excalidraw]]` and PDFs `![[Paper.pdf]]` are shown
inside the editor; the link text comes back while the cursor is in the link. The pictures in
this guide are embedded exactly like this. Switch it off in Settings → Appearance
("Show embedded images inside the editor").

**Captions.** Every embed that is shown has its link as a small caption below it: the file's
link (`z_media/photo.png`), or the words you give it after a `|`: `![[photo.png|Sea at dusk]]`
(and `![[photo.png|Sea at dusk|300]]` with a width). Click the caption to open the file. The
[[PDF]] export draws the same thumbnails with the same captions.

**Resizing.** Pictures and videos can be resized by dragging; see [[Editor]].

# Moving and renaming

![[tree-drag.png]]

Drag notes, drawings, files and folders in the file list into another folder. Rename a note from
its menu. In both cases links in other notes (and in drawings) are updated, so nothing breaks.
A name with a character that does not work on Windows (such as `:` or `?`) is not refused: the file gets
a name without it and the message says so ([[Vault settings]]).

# The note menu

![[more-menu.png]]

Export to PDF ([[PDF]]), insert a new drawing, rename, or move to the trash. The trash keeps
the original folder (`.trash/<date>/<path>`), and "Restore last deleted" in the file list's
context menu brings the last item back.

# Go back and forward

Mouse side buttons or Alt+← / Alt+→ (macOS: Cmd+[ / Cmd+]). Sort the file list by name, date modified or date
created (the sort icon above the list).

Next: [[Editor]], [[Tabs]], [[Search and pickers]].

#userguide #notes

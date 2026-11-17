---
features:
  - "Keyboard only: sidebar, file list and vim-style editing"
---
# Editor

How you write in Shale. Back to [[Start here]]; the links and embeds are in [[Notes and links]].

## Lists and lines

- **Enter** continues a list (also numbered lists and to-dos `- [ ]`).
- **Tab / Shift+Tab** indent list items, or change heading levels.
- **Alt+↑ / Alt+↓** move the current line.
- A gutter on the left shows **line numbers** (Settings → General → Appearance, on by default);
  a wrapped line keeps one number.

## Formatting toolbar

![[editor-colours.png]]

A row of buttons centred above the text of a note (not in the reading view): **B**old, *I*talic,
~~S~~trikethrough, `==` highlight, `<>` inline code; the highlight colour and font colour
palettes; a **H** menu for headings (Normal text, Heading 1 to 6); bullet list, numbered list,
task, quote; code block, horizontal rule, table; `[[ ]]` note link, `[ ]( )` web link, `![[ ]]`
embed (it opens the `[!` file picker of [[Notes and links]]); and "Clear formatting".

- With text selected, an inline button wraps it in the markers; click it again and they come
  off. With nothing selected it puts the markers in and the cursor between them.
- A line button (heading, list, task, quote) works on every line the selection touches, and
  toggles off when they all have it. A heading replaces a list marker and the other way round.
- Every click is one undo step, and the keyboard stays in the note, with the Markdown preview on
  or off and with vim-style editing (insert mode stays, otherwise you are in normal mode).
- Settings → General → Editing, "Show formatting toolbar" turns the toolbar off; "Toolbar button size"
  makes its buttons bigger or smaller (80 % to 200 %, 110 % by default).

## Colours

The **highlight colour** button opens a palette of four neon colours (yellow, green, cyan, magenta),
"No highlight", and "Custom colour…" with a colour picker. The **font colour** button does the same
with your own preset colours: edit them under "Font colours" in Settings → General → Editing (the
highlight presets are there too). A new colour on text that already has one replaces it.

The colours are stored in the note as plain HTML, the way Obsidian's colour plugins write it, so
other editors and renderers show them too:

```
<mark style="background: #FFFF00;">highlighted</mark> and <span style="color: #E53E3E;">red text</span>
```

`==text==` stays the plain highlight. Shale also reads `background-color:`, short `#RGB` and
`#RRGGBBAA` colours, and single quotes, but always writes this form. A tag without its closing
tag, or inside inline code, is just text. Colours show in the editor (the tags dimmed; the
Markdown preview hides them), in the [[Reading view]], in table cells and in the [[PDF]] export.

## Markdown preview

![[editor-preview.png]]

![[preview-toggle.webm]]

Turn it on with Settings → General → Appearance, or the eye icon in the top right of the
editor (off by default). Every line is drawn rendered: headings at their size, bold, italic,
strike and code without their markers, links as just their text. Only the line the cursor is in
shows the raw markdown; with "Raw lines around the cursor" that many lines before and after do
too (or all the lines of a selection). The note stays editable: links still open with a click
and task boxes still tick.

A table (`| a | b |` rows with a `|---|---|` line below the header) away from the cursor is drawn as
a real grid: borders, a bold header row, columns aligned as `:---`, `:---:` or `---:` say, code,
bold and links inside the cells, and long text wrapping inside its own column.

Click a cell to edit it right in the grid: the cell shows its own markdown in a small box, and
what you type goes into the note at once (a `|` you type is written as `\|`). **Tab** and
**Shift+Tab** go to the next and previous cell, **Enter** and **Shift+Enter** to the cell below
and above, **Esc** (or a click elsewhere) leaves. The arrow keys move inside the cell's text
and, at its edge, on to the next cell: **→** at the end of the text goes to the next cell (from
the last column to the next row), **←** at its start to the previous one, **↑** on the first line
of the text to the cell above and **↓** on its last line to the cell below. Everything from the click to leaving is one
step you can undo. Moving the cursor into the table with the keyboard still shows its raw rows.

![[table-cell-edit.png]]

If you prefer the raw markdown, turn off "Edit table cells in the grid" in Settings → General →
Appearance: a click on a table then shows its raw rows to edit. A single table can be switched
either way by the small button above the top right corner of its grid (**md**: edit this table
as raw markdown; the grid icon: edit its cells in the grid).

Drag a border between two columns to make the left one wider or narrower, the border under a
row to make it taller, and the right edge, the bottom edge or the bottom right corner to resize
the whole table. Double-click a column border to give all columns the same width (sharing the
table's current width), or a row border to give all rows the same height.

The switch and the sizes are kept in the note, in a comment line right above the table, e.g.
`<!-- shale-table edit=raw cols=160,90 rows=30,30,44 -->`. It is hidden with the grid, in the
reading view and in the PDF (which uses the sizes); other apps such as Obsidian ignore HTML comments.

While the pointer is over the grid, small buttons appear: a **+** right of the header adds a
column at the end, a **+** bar under the table adds a row at the end, and for the row and the
column under the pointer a **+** adds one after it and a **×** deletes it (the header row, the
last row and the last column stay). Each click changes only the table's lines, as one step you
can undo, and the table stays a grid.

![[table-grid.png]]

## Sound players

![[sound-player.png]]

A sound embedded with `![[song.mp3]]` (mp3, ogg, oga, flac, m4a, aac and wav; opus files are
recognised but cannot be played yet) shows a player in the editor, the markdown preview and the
reading view. It shows the cover from the file's tags, or a tile with the file name. The round
button over it plays and pauses (so does Space once you have clicked the player); click the bar
to jump, or drag it and let go to seek. The speaker button mutes, the slider sets the volume for
all players. Only one sound plays at a time; it goes on while you scroll or switch between the
editor, the preview and the reading view, and stops when the note's tab is closed or another
vault is opened. Nothing ever starts by itself. How opaque the play button is (60 % by
default) is set in Settings → General → Appearance, see [[Settings reference]].

## Resizing pictures and videos

Move the mouse over a picture or a video in the editor: its edges and corners
become handles, and a drag changes the size (the proportions stay). The width is written into
the link when you let go: `![[photo.png|300]]` (points; this is also what Obsidian writes), and
the reading view shows the embed at that width. Two round buttons sit in the corner of the
picture, looking like the play button of the sound player: **100%** sets the natural size, and the
fullscreen button shows the picture (or the video, with its controls) over the whole window; Esc,
the cross or a click beside it closes it.

![[image-resize.png]]

![[image-resize.webm]]

Videos resize the same way (the controls stay below the picture; **100%** is the video's own size, and fullscreen keeps the controls):

![[video-resize.webm]]

## Vim-style editing

Settings → General → Appearance, off by default. Notes open in normal mode.

| Keys | What it does |
|---|---|
| h j k l, w b, W B, 0 $, gg G | move |
| `:12` | jump to line 12 |
| `/text`, n, N | search and go on |
| i a I A o O | start typing; Esc stops |
| x | delete a character |
| v, V | select with the same motions; d cuts, y copies, p pastes |
| dd | delete the line (onto the clipboard; p puts it back below) |
| u, Ctrl+R | undo, redo: up to 200 steps per note, an insert session is one step; kept in the vault for 30 days, so it works after a restart ([[Data usage and caches]]) |
| gt | follow the link under the cursor like a click (a note link or a web address) |

The status bar shows NORMAL, INSERT, VISUAL or V-LINE. The vim keys themselves cannot be
rebound. In normal mode **Space Tab** goes back to the view this tab showed just before, and
again to return. **Space f f** and **Space f g** open the pickers of [[Search and pickers]].

## Keyboard only

![[keybindings.png]]

Ctrl+H puts the keyboard into the left sidebar, Ctrl+L back into the note. Ctrl+Shift+B opens
the file list with the open note selected: j / k go from file to file, Ctrl+J / Ctrl+K from
folder to folder, Space opens or closes a folder, Enter opens a file. All of this can be
changed in Settings → Keybindings, see [[Keyboard shortcuts]].

## Text size and width

Text size (12 to 24) and "Readable line length" are in Settings → General → Appearance, see
[[Settings reference]].

#userguide #editor

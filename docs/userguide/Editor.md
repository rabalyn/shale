---
features:
  - "Keyboard only: sidebar, file list and vim-style editing"
---
How you write in Shale. Back to [[Start here]]; the links and embeds are in [[Notes and links]].

## Lists and lines

- **Enter** continues a list (also numbered lists and to-dos `- [ ]`).
- **Tab / Shift+Tab** indent list items, or change heading levels.
- **Alt+↑ / Alt+↓** move the current line.
- A gutter on the left shows **line numbers** (Settings → Editor, on by default);
  a wrapped line keeps one number.
- The **cursor** has a shape of your choice (Settings → Editor → Cursor): a bar before the character at the
  cursor (the default), a bar after it, a block or an underline. With vim-style editing the cursor in
  normal mode has its own choice (Settings → Vim, default: block). "Blinking cursor" turns the blinking off.

## The title line and pasting

Above the first line a note shows its **title line**: the file name without `.md` and without a
leading date prefix `YYMMDD_` (a note called `261117_Plan.md` shows `Plan`). Edit it to rename the
note; the prefix stays and links in other notes are updated. Tabs show the name without the prefix
too; the file list and links keep the full name. Settings → Editor → "Date prefix for new notes"
(on by default) names the notes Shale makes `YYMMDD_<name>` with today's date. The title line is not
part of the text, so a `#` heading in the note is an ordinary heading, numbered like the others.

Pasting text whose headings are all `##` or deeper asks "The pasted text has no # heading. Lift its
headings?": **Lift** moves them up so that the highest becomes `#`, **Paste as it is** (or Esc) pastes
the text unchanged. Text without headings, or with a `#`, is pasted at once.

## Formatting toolbar

![[editor-colours.png]]

A row of buttons centred above the text of a note (not in the reading view): **B**old, *I*talic,
~~S~~trikethrough, `==` highlight, `<>` inline code; the highlight colour and font colour
split buttons (apply the current colour, ▾ for the palette); a **H** menu for headings (Normal text, Heading 1 to 6); bullet list, numbered list,
task, quote; code block, horizontal rule, table; `[[ ]]` note link, `[ ]( )` web link, `![[ ]]`
embed (it opens the `[!` file picker of [[Notes and links]]); and "Clear formatting".

- With text selected, an inline button wraps it in the markers; click it again and they come
  off. With nothing selected it puts the markers in and the cursor between them.
- A line button (heading, list, task, quote) works on every line the selection touches, and
  toggles off when they all have it. A heading replaces a list marker and the other way round.
- Every click is one undo step, and the keyboard stays in the note, with the Markdown preview on
  or off and with vim-style editing (insert mode stays, otherwise you are in normal mode).
- Every button has an Alt key: hold **Alt** and a small badge with the key shows at each button
  (Alt+B bold, Alt+I italic, Alt+E the heading menu, Alt+V edit / reading view, Alt+P Markdown
  preview, Alt+N new tab, Alt+1 … Alt+0 the ribbon icons, Alt+, and Alt+. the sidebars; the list is
  in [[Keyboard shortcuts]]). A key does what a click does, with the keyboard staying in the note,
  and does nothing where the button is not there or greyed out. The keys can be changed in
  Settings → Keybindings (scope Note view); on macOS they have no key until you set one.
- Settings → Editor → Editing, "Show formatting toolbar" turns the toolbar off; "Toolbar button size"
  makes its buttons bigger or smaller (80 % to 200 %, 110 % by default).

## Colours

The **highlight colour** and **font colour** buttons are split buttons. A click on the coloured part
applies the colour it shows (neon yellow and red at start, then the last colour you picked) to the
selection at once. The small ▾ beside it opens a palette; a colour picked there is applied and becomes
the button's colour. The highlight palette has four neon colours (yellow, green, cyan, magenta),
"No highlight", and "Custom colour…" with a colour picker; "No highlight" and "Default colour" take the
colour off and keep the button's colour. The **font colour** palette does the same
with your own preset colours: edit them under "Font colours" in Settings → Editor → Editing (the
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

## Footnotes, underline, subscript and superscript

A footnote is a reference in the text and a definition anywhere in the note:

```
The claim[^1] is old, as H<sub>2</sub>O and x<sup>2</sup> are, and <u>this</u> matters.

[^1]: The source of the claim
    with a second line, indented by four spaces or a tab.
```

A reference `[^label]` (letters, digits, `-` and `_`) with a definition shows only its label, raised
and small, in the link colour; the brackets and the `^` are hidden by the Markdown preview. The
definition `[^label]: text` at the start of a line shows its label raised and its text a little
smaller; lines after it that start with four spaces or a tab belong to it. With the cursor in a line
you see its raw text, as with every other markup. A reference without a definition is shown as
written. In the [[Reading view]] a click on a reference scrolls to its definition, and a click on
the label of a definition scrolls back to the first reference. Links, `#tags` and formatting work
inside a definition. Footnotes in inline or fenced code are just text.

`<u>…</u>` underlines, `<sub>…</sub>` lowers and `<sup>…</sup>` raises its text, both at about 70% of
the size. They follow the rules of the colour tags above: any letter case, they nest with `<mark>`,
`<span>` and each other, and a tag without its partner or inside inline code is plain text. The tags
are hidden by the Markdown preview. In a table cell, `<br>` (also `<br/>` and `<br />`) starts a new
line in the cell, where the table is drawn as a grid; the cell editor keeps the tag as you wrote it.

All of this also shows in the heading titles of the outline, in table cells, and in the [[PDF]]
export, where a footnote reference links to its definition. Word notes imported with pandoc use
exactly these forms (see [[Import]]).

## Math

A formula between dollar signs is set in the text, `$E = mc^2$`, and two dollar signs make a display formula, centred in a row of its own:

```
The roots of a quadratic are $x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$.

$$
\sum_{k=1}^{n} k^2 = \frac{n(n+1)(2n+1)}{6}
$$
```

![[math-editor.png]]

The opening `$` is not followed by a space, the closing one is not preceded by a space and not followed by a digit, and both are on one line, so `costs $5 and $10` is no formula; `\$` is a plain dollar sign. A display formula is `$$…$$` on one line, or `$$` on lines of their own around one or more lines, or a ```` ```math ```` block. Nothing in code or in the front matter is a formula.

Shale typesets formulas itself, with the bundled font STIX Two Math (SIL Open Font License, its licence is beside it in the program's sources) and no TeX installed. It knows letters in math italic, digits and operators upright, `{}` groups, `^` and `_` (also both, and nested), `\frac`, `\dfrac`, `\tfrac`, `\binom`, `\sqrt` and `\sqrt[n]`, Greek letters, the usual relations, binary operators, arrows and symbols, big operators with limits (`\sum`, `\prod`, `\int`, `\iint`, `\oint`, `\lim`; the limits stand below and above in display style and beside the symbol in the text; `\limits` and `\nolimits` change that), function names (`\sin`, `\log`, `\max`, `\operatorname{…}`), `\left…\right` delimiters that grow with their content, accents (`\hat`, `\bar`, `\vec`, `\dot`, `\tilde`, `\overline`, `\underline`), `\text{…}`, `\mathrm`, `\mathbf`, `\mathit`, `\mathbb`, `\mathcal`, the spaces `\,` `\:` `\;` `\quad` `\qquad` `\!`, and the environments `matrix`, `pmatrix`, `bmatrix`, `vmatrix`, `cases` and `aligned` (rows `\\`, columns `&`).

With the Markdown preview on, a formula takes the place of its TeX; the line with the cursor shows the TeX, as with every other markup (the TeX stays in the note, so the cursor, selection and search work on it). The same formulas are drawn in the [[Reading view]], in table cells, in the headings of the outline and in the [[PDF]] export. A formula Shale cannot typeset (an unknown command, braces that do not match, `\left` without `\right`, more than 64 levels of nesting or more than 10 000 characters) stays as its TeX in the code style, and the reason shows when you point at it. Word files imported with pandoc bring their formulas as `$…$` and `$$…$$` (see [[Import]]).

## Code

Code is coloured: write the language after the opening fence, as in ```` ```js ````
or ```` ```rust ```` (also `python`, `sh`, `json`, `yaml`, `typescript`, `tsx`, `toml` and about 140 more; the usual short names such as `rs`,
`py` and `yml` work too), and keywords, strings, numbers, comments, functions and types get their
own colours, in the editor, in the reading view and in the [[PDF]] export. A block with no language, or one Shale does
not know, stays plain. Inline code (`console.log("hi")`) has no language, so only its strings,
numbers and called names are coloured.

## Markdown preview

![[editor-preview.png]]

![[preview-toggle.webm]]

Turn it on with Settings → Editor, or the eye icon in the top right of the
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

While a cell is edited, the [[#Formatting toolbar]] works on the cell: bold, italic, strikethrough,
highlight, code, links, colours and "Clear formatting" format the selected text in the cell (or
put the markers in at the cursor), and the change is part of the cell's undo step. The buttons
for headings, lists, tasks, quotes, code blocks, rules, tables and embeds are greyed out then,
because they make no sense inside a cell.

**Shift+arrow keys** select several cells: from the cell you edit, Shift+→ and then Shift+↑ select
a block of 2 by 2 cells (it can span rows and columns, and shrinks again with the opposite arrow).
Inside a cell's text the Shift+arrow keys select text first; at the edge of the text they go on to
the cells. All selected cells are highlighted. A toolbar button (bold, italic, …) formats every
selected cell, **Delete** or **Backspace** clears all of them, and both are part of the editing's
one undo step. Typing ends the selection: the text goes into the top left cell of the block. A
plain arrow key (it moves one cell on from the cell you started in) or **Esc** (it keeps that cell
edited) ends the selection too.

![[table-cell-edit.png]]

![[table-cell-block.png]]

The clip selects a block with Shift+→ and Shift+↓, makes it bold with the toolbar and clears it
with Backspace:

![[table-cell-block.webm]]

If you prefer the raw markdown, turn off "Edit table cells in the grid" in Settings → Editor: a click on a table then shows its raw rows to edit. A single table can be switched
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

A sound embedded with `![[song.mp3]]` shows a player in the editor, the markdown preview and the
reading view; videos work the same way. All about them is in [[Sounds and videos]].

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

Settings → Vim (listed under Editor), off by default. Notes open in normal mode. The cursor in normal mode is a block
unless you choose another shape ("Cursor in normal mode"); typing uses "Cursor while typing" (Settings → Editor).
When you press Esc to leave insert mode, the cursor stays on the character right after where you stopped
typing; "Esc from insert mode" can make it step back one character like Vim does.

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
| Shift+K | preview the link under the cursor (see below) |

**Link preview.** With the cursor on a link, **Shift+K** shows a floating window (640 x 480 points at first) below
the cursor, like neovim's hover. A note link shows the note as the reading view draws it, from the
heading when the link has one (`[[Note#Heading]]`); a note that does not exist yet says so; a picture
is shown; a web link shows the page's title, its description and the start of its text. The page is
loaded in the background (only `https://`, at most 30 seconds and 1.5 MB, remembered for this
session), so the window never waits: it says "Loading …" first, and says why when the page could not be
read. The editor keeps the keyboard: moving the cursor, typing or **Esc** closes the preview. Press
**Shift+K** again and the keyboard moves into the preview: **j / k** or **↓ / ↑** scroll,
**Ctrl+D / Ctrl+U** or **PageDown / PageUp** scroll a page, **Enter** opens the link exactly as `gt`
does (a missing note is created), **Ctrl+Enter** opens it in a new pane to the right of this
one (see [[Tabs]]; a web link opens in the browser as with Enter), and **Esc** closes the preview and gives the keyboard back to the
editor, with the cursor where it was and the text unchanged. Off a link, Shift+K only says so in the
status bar. The key is one of the definable ones (Settings → Keybindings → Vim-style editing, normal
mode): you can give it, for example, **Space Space**.

The preview's size can be changed: drag one of its four edges or four corners (the mouse pointer
turns into a resize arrow). The preview stays open while you drag, and the editor's cursor stays where
it is. The size is a setting that every preview uses and that is still there after a restart:
Settings → Vim → "Link preview size" shows it as two numbers (width and height in points;
at least 240 x 160, and never bigger than the window) with a **Reset size** button, see [[Settings reference]].

The status bar shows NORMAL, INSERT, VISUAL or V-LINE. The vim keys themselves cannot be
rebound. In normal mode **Space Tab** goes back to the view this tab showed just before, and
again to return. **Space f f** and **Space f g** open the pickers of [[Search and pickers]].

## Keyboard only

![[keybindings.png]]

Ctrl+H (macOS: Cmd+Shift+H) puts the keyboard one place to the left (with split panes: into the pane on the left,
from the first pane into the left sidebar), Ctrl+L one place to the right (from the sidebar into
the first pane, then pane by pane; it stays in the last one). Ctrl+Shift+B opens
the file list with the open note selected: j / k go from file to file, Ctrl+J / Ctrl+K from
folder to folder, Space opens or closes a folder, Enter opens a file. All of this can be
changed in Settings → Keybindings, see [[Keyboard shortcuts]].

## Text size and width

"Readable line length" is in Settings → Appearance. The font and size of the raw markdown text and
of the rendered text (the preview and the reading view) are two settings of their own in
Settings → Fonts, see [[Fonts]] and [[Settings reference]].

#userguide #editor

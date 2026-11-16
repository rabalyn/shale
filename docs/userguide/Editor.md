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

## Markdown preview

![[editor-preview.png]]

![[preview-toggle.webm]]

Turn it on with Settings → General → Appearance, or the eye icon in the top right of the
editor (off by default). Every line is drawn rendered: headings at their size, bold, italic,
strike and code without their markers, links as just their text. Only the line the cursor is in
shows the raw markdown; with "Raw lines around the cursor" that many lines before and after do
too (or all the lines of a selection). The note stays editable: links still open with a click
and task boxes still tick.

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

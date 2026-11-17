---
features:
  - "Fonts and text sizes"
---
![[settings-fonts.png]]

Settings → Fonts sets the font and the text size of six areas of the window:

- **Dialogs and menus**: also the sidebars and everything not listed below.
- **Editor: raw markdown**: the markdown text as you type it, and the lines around the cursor that
  stay raw in the markdown preview.
- **Editor: rendered text and reading view**: the lines the preview draws rendered, and the reading
  view. It can have another font and size than the raw text.
- **Mail**, **Calendar** (also the calendar of the right sidebar) and **Feeds**.

**Use the same font and size everywhere** is on at first: one row, one font and size for everything
(the editor text is drawn a little larger than a dialog's, as it always was). Switch it off to get a
row per area; each starts with what it had.

To choose a font, press the button of a row. A list of the fonts installed on the computer opens
with a search field on top: type a part of the name, move with the arrow keys, press **Enter** to pick
(a click works too) and **Esc** to close without picking. **Default** is Shale's own font. The list
opens on the font the row has.

Next to every row is a sample text, "The quick brown fox, **bold** and *italic*", in the row's
font and size, so you see all the fonts you have chosen side by side. While a list is open, the
sample of its row shows the font the mouse is over (or the one you moved to with the arrow keys)
without picking it; close the list and it shows the chosen font again. A font you have not used yet
takes a moment to load ("Loading the font…"). Large sizes are shown at 28 points at most. The
list is read in the background the first time it is needed, so it may take a moment on a computer with
very many fonts. The size is in points, 8 to 48; code stays in a monospace font.

Bold and italic text (`**bold**`, `*italic*`, quotes) is drawn in the chosen font's own bold, italic
and bold-italic faces when they are installed, as for most fonts that come in a family of styles. A
font that has only a regular face (and Shale's own font) gets a slanted or wider-spaced imitation
instead; bold text is also a little brighter either way.

A font that is no longer installed (or comes with settings from another computer) is replaced by
Shale's own font, you are told once, and the row says "not installed". The name stays in the settings
and is used again when the font is installed.

Everything changes at once, also when you edit the `[fonts]` table of the settings file, see
[[Settings reference]]. Back to [[Start here]]; the editor is in [[Editor]].

#userguide #settings

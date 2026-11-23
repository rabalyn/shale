---
features:
  - "Drawings"
---
![[drawing.png]]

Hand-drawn style sketches, saved as standard `.excalidraw` files. The drawing on
[[Start here]] is one: [[How notes connect.excalidraw]].

# Draw

- Tools on keys 1 to 0: hand, select, rectangle, diamond, ellipse, arrow, line, pen, text,
  eraser.
- The style panel sets stroke and fill colours, fill style, stroke width, outline pattern,
  sloppiness, corners, label font, size, colour, background and alignment, opacity, and the
  order of shapes.
- Double-click a shape to label it. Arrows snap to shapes and follow them when shapes move.
  Hold Shift to keep lines at 45 degree steps.
- Select several shapes (Shift+click, or drag a frame around them) and drag a corner of the box
  around them: every shape, line, arrow, scribble and text in it scales from the opposite corner.
  Undo takes the whole resize back in one step. A single line or arrow is changed by its ends.
- "Auto arrange" untangles a diagram; "Spread out" and "Pull together" change the spacing.
- Insert a new drawing from the note menu, or embed one with `![[Name.excalidraw]]`.

# Links inside drawings

Type `[[` in a label or text for note suggestions, `[[p ` for people and `[!` for the file
picker. A shape with a link gets a small ↗; click it (or Ctrl+click the shape) to open the
note, Shift for a new tab. The linked note lists the drawing under Backlinks, and the graph
shows the connection ([[Graph and backlinks]]).

# Obsidian files

- Drawings of the Obsidian Excalidraw plugin (`.excalidraw.md`) open in the drawing editor and
  are saved in the plugin's own format, so Obsidian keeps reading them.
- Pictures inside drawings show up, also in embedded drawings and in PDF export. A picture of an Obsidian drawing (`.excalidraw.md`) may live outside the vault; **Copy outside media into the vault** in the drawing's ⋯ menu copies it into the media folder and points the drawing at the copy, leaving the rest of the file as it was.

## Obsidian canvases

![[canvas.png]]

`.canvas` files open in the drawing view with cards, groups, file and web cards and labelled
arrows. Opening one asks whether to convert it:

![[canvas-convert.png]]

**Convert** makes an editable `Name.excalidraw.md` next to it; **Open read-only** just shows
it. The canvas file itself is never changed. Settings → Editor → Drawings ("Convert Obsidian
canvases … instead of asking") skips the question. "Dark drawing canvas" is in Appearance.

#userguide #drawings

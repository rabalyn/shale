**Ctrl+E** switches between editing and the reading view of a note. Back to [[Start here]].

![[reading.png]]

The reading view shows the note rendered: headings, lists, links (blue and red as in the
editor), embedded pictures, drawings and PDFs, and task boxes you can tick (the change is saved
in the note). Coloured highlights and font colours (`<mark style="background: …">`, `<span style="color: …">`, see [[Editor]]) are shown with their tags hidden. Footnote references are small raised labels: a click on one scrolls to its definition, a click on the label of a definition scrolls back (see [[Editor]]). Formulas between dollar signs are typeset: `$…$` in the text, `$$…$$` on lines of their own and fenced code blocks with the language `math` centred (see [[Editor]]), also in table cells; one Shale cannot typeset stays as its TeX in the code style and says why when you point at it. `<u>`, `<sub>` and `<sup>` give underlined, lowered and raised text, and `<br>` in a table cell starts a new line. Tables are drawn as grids, with a bold header row and aligned columns. Headings carry their numbers when numbering is on, and a link to a heading (`[[Note#Heading]]`) scrolls the reading view to it without leaving it (see [[Notes and links]]). The properties at the top of a note (the front matter between two `---` lines) are drawn as a card made for the kind of note instead of raw lines: an imported mail as a mail header (sender in bold, date at the right (a link that shows that day in the calendar), "to …", tags as chips), an event with its time (a link to its day in the calendar), place, attendees and a "repeats" badge, a Word import with one chip for its page ("A4 · 2.5 cm margins · 11 pt"), any other note as key and value rows. Names that are notes (your contacts) are links. Ids and print settings are left out; the small ▸ under the card shows every property. **Settings → Appearance → Properties in the reading view** changes this to a one-line summary or the raw lines. Fenced code blocks are drawn as boxes with the language in a badge at the top right (see [[Editor]]); point at one and a copy icon left of the badge copies its code. Blocks fold here as in the editor (see [[Editor]]): hover the first line of a heading section, quote, list item, list, code block or signature and click the small ▾ left of it, or click the `… n lines` badge under a folded block (a folded code block is one row, its badge at the left of the box; in a mail the quotes say `… n quoted lines`). The folds are the editor's: fold a section while editing and it is folded here, and the other way round, for as long as Shale runs; nothing is written into the note. **Ctrl+Shift+O** folds every block and **Ctrl+Shift+I** unfolds all; the keys that fold at the cursor work in the editor only. A click on a footnote mark, or on a heading in the outline, opens the fold that hides its target. The properties card at the top has its own ▸ toggle. If you only want to read, this is the quiet way to do it. The always-rendered
alternative while you edit is the Markdown preview in [[Editor]].

![[reading-folds.png]]

![[table-reading.png]]

![[math-reading.png]]

#userguide #reading

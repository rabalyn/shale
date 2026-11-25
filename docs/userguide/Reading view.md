**Ctrl+E** switches between editing and the reading view of a note. Back to [[Start here]].

![[reading.png]]

The reading view shows the note rendered: headings, lists, links (blue and red as in the
editor), embedded pictures, drawings and PDFs, and task boxes you can tick (the change is saved
in the note). A picture embedded inside a line of text (`an icon ![[icon.png]] here`, also in a table cell) is drawn in the line, as high as the text of the line and sitting on its baseline like a letter, never wider than the column; the pointer and a click on it work as on the link. A picture that is missing or cannot be read, and any other embed inside a line (a drawing, a PDF, a sound), stay their link text, and an embed on a line of its own is shown as a block as before. Coloured highlights and font colours (`<mark style="background: …">`, `<span style="color: …">`, see [[Editor]]) are shown with their tags hidden. Footnote references are small raised labels: a click on one scrolls to its definition, a click on the label of a definition scrolls back (see [[Editor]]). Formulas between dollar signs are typeset: `$…$` in the text, `$$…$$` on lines of their own and fenced code blocks with the language `math` centred (see [[Editor]]), also in table cells; one Shale cannot typeset stays as its TeX in the code style and says why when you point at it. `<u>`, `<sub>` and `<sup>` give underlined, lowered and raised text, and `<br>` in a table cell starts a new line. Tables are drawn as grids, with a bold header row and aligned columns. Headings carry their numbers when numbering is on, and a link to a heading (`[[Note#Heading]]`) scrolls the reading view to it without leaving it (see [[Notes and links]]). The properties at the top of a note (the front matter between two `---` lines) are drawn as a card made for the kind of note instead of raw lines: an imported mail as a mail header (sender in bold, date at the right (a link that shows that day in the calendar), "to …", tags as chips), an event with its time (a link to its day in the calendar), place, attendees and a "repeats" badge, a Word import with one chip for its page ("A4 · 2.5 cm margins · 11 pt"), any other note as key and value rows. Names that are notes (your contacts) are links. Ids and print settings are left out; the small ▸ under the card shows every property. **Settings → Appearance → Properties in the reading view** changes this to a one-line summary or the raw lines. Fenced code blocks are drawn as boxes with the language in a badge at the top right (see [[Editor]]); point at one and a copy icon left of the badge copies its code. Blocks fold here as in the editor (see [[Editor]]): hover the first line of a heading section, quote, list item, list, code block or signature and click the small ▾ left of it, or click the `… n lines` badge under a folded block (a folded code block is one row, its badge at the left of the box; in a mail the quotes say `… n quoted lines`). The folds are the editor's: fold a section while editing and it is folded here, and the other way round, for as long as Shale runs; nothing is written into the note. **Ctrl+Shift+O** folds every block and **Ctrl+Shift+I** unfolds all; the keys that fold at the cursor work in the editor only. A click on a footnote mark, or on a heading in the outline, opens the fold that hides its target. The properties card at the top has its own ▸ toggle. If you only want to read, this is the quiet way to do it. A picture, PDF or player whose file is outside the vault ([[Sounds and videos]]) has **Copy into the vault**: a button beside **Open** on a picture or PDF, a line under a player. The always-rendered
alternative while you edit is the Markdown preview in [[Editor]].

![[reading-folds.png]]

![[table-reading.png]]

![[math-reading.png]]

# Image carousel

Several pictures that belong together, such as the layers of a keyboard or the steps of a setup, can be a carousel: a quote whose first line is `[!carousel]` and whose other lines are each one picture. In the reading view it shows **one picture at a time**, at the size its link gives it, with its caption (the text after the `|` of the link, or the alt text of `![alt](file.png)`), the buttons **Previous image** and **Next image**, a dot per picture and "2 / 3". Click a dot to go to that picture. **← and → work while the pointer is over the carousel**, and the pictures go round: after the last comes the first. Which picture is shown is not saved in the note, and each carousel of a note has its own.

```markdown
> [!carousel] Calendar views
> ![[calendar-month.png|Month]]
> ![[calendar-week.png|Week]]
> ![[calendar-sidebar.png|In the sidebar]]
```

Here it is, made from the pictures of this guide:

> [!carousel] Calendar views
> ![[calendar-month.png|Month]]
> ![[calendar-week.png|Week]]
> ![[calendar-sidebar.png|In the sidebar]]

The note stays plain Markdown: Obsidian and GitHub show the same lines as a quote with all the pictures, and Shale's exports (PDF, LaTeX, Word and the Hugo site, see [[PDF]] and [[Publishing a vault]]) print or publish every picture in order, the title in bold and each caption below its picture. A `[!carousel]` line with no picture after it, and any other quote or callout, stays an ordinary quote. The editor has the same carousel in its rendered state: one picture with the buttons below it. While the cursor is in the quote its lines are shown as written so you can edit them, and ← and → work under the pointer only while the text does not have the keyboard.

#userguide #reading

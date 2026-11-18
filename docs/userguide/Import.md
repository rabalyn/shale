---
features:
  - "Importing e-mails, events and files (for project tracking)"
---
Turn mails, events and files into notes. Back to [[Start here]].

![[import-dialog.png]]

- Drag e-mails from Thunderbird, events from a calendar, `.eml` or `.ics` files, or any other
  files (plans, PDFs, …) onto the window. The ribbon's Import button opens the same dialog; so
  do **Import to vault** in [[Mail]] and **Save to vault** in [[Feeds]].
- A `.md` or `.markdown` file becomes a note with its text unchanged ("Markdown note"). When it has headings
  but no `#`, a ticked box "Lift headings (## → #, ### → ##, …)" moves them all up so that the highest
  becomes `#` (code and front matter stay as they are).
- A `.docx` file (Word) becomes a note too ("Word document"), when you have turned on **Settings →
  Import → Import Word files as notes** and pandoc is installed (see below). Otherwise it is saved as an
  attachment, as before; if the box is ticked but pandoc is missing, the dialog says "pandoc was not found
  (Settings → Import)".
- Choose the folder (one per project, for example) and tags. Small pictures from signatures are
  left out by default, and each attachment can be ticked on or off. Nothing is written until you
  press Import.
- Drop onto a folder in the file list to import into it, or onto a note to also add links to the
  new notes there.
- Messages that were imported before are recognised and left out unless you tick them again.

![[drag-event.png]]

Each e-mail becomes a note named "date subject" with sender, recipients and date, tags in its
properties, links to the attachments and the original message, and the text.

![[imported-note.png]]

## Word files

![[settings-import.png]]

1. Install **pandoc**: Linux `sudo pacman -S pandoc-cli` or `sudo apt install pandoc`, macOS
   `brew install pandoc`, Windows `winget install JohnMacFarlane.Pandoc`, or <https://pandoc.org/installing.html>.
2. In **Settings → Import** tick "Import Word files as notes (needs pandoc)". The page shows where pandoc was
   found and its version. If it is somewhere unusual, type the path in "pandoc program"; **Check again**
   looks once more after an install.
3. Drop a `.docx` on the window. Import runs pandoc in the background (a file that takes longer than
   two minutes is stopped).

The note is made in the folder of the dialog. It keeps headings, bold, italic, underline, strike-through,
sub- and superscript, links, lists, tables (tables with merged cells become plain tables, a cell with
several lines is shown with line breaks), quotes, footnotes, math (written as `$…$` and `$$…$$`, typeset as [[Editor]] says) and highlights (`==text==`). A picture
becomes an embed with its caption in italics below it. A box "Lift headings" does what it does for a Markdown
file, for a document without a `#` heading.

The look of the document comes along as far as Shale can show it: the colour of text (`<span style="color: …">`)
and of highlights other than plain yellow (`<mark style="background: …">`, see [[Editor]]), a title and
subtitle (a `#` heading and an italic line). The page and the body are written into the note's properties
for the [[PDF]] export: `print-paper` (A4, A5, Letter, Legal or `<width>x<height>mm`, wider than tall is
landscape), `print-margins` (top, right, bottom, left in mm), `print-font` (serif, sans or mono),
`print-font-size` (pt), `print-line-spacing` (a factor such as 1.15), `print-paragraph-spacing` (pt after a
paragraph), `print-align` (left or justify), `print-heading-color`, `print-title` (always `false`, the title
is in the text) and `print-toc` (`false` unless the document has a table of contents). Edit them like any
property. Centred and right-aligned paragraphs are not kept (issue 340).

Everything one Word file brings goes into one folder, `z_media/<year>/<month>/import_docx_<name>/`, named
after the file (spaces become `-`; a number is added when the name is taken): the pictures in `media/` and, with
"Keep the original .docx" ticked, the original, which the note names in its properties as `source`. If pandoc
cannot read the file, nothing is written and a message names the file and pandoc's first error line.

People and tags are found automatically: [[Tags and people]]. On Wayland Shale reads what
Thunderbird hands over; when that is only a link to the message, Shale finds the message in
Thunderbird's local copy (IMAP accounts need "Keep messages for this account on this
computer").

#userguide #import

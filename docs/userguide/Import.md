---
features:
  - "Importing e-mails, events and files (for project tracking)"
---
Turn mails, events and files into notes. Back to [[Start here]].

![[import-dialog.png]]

- Drag e-mails from Thunderbird, events from a calendar, `.eml` or `.ics` files, or any other
  files (plans, PDFs, …) onto the window. The ribbon's Import button opens the same dialog; so
  do **Import to vault** in [[Mail]] and **Save to vault** in [[Feeds]].
- **Add files…** (below the list) opens the file picker: choose one file or several at once; the
  kinds of the import are the filters (mail, events, notes, pictures, PDFs, sounds and videos, Word
  files). Your system's own dialog opens first; see [[Vault settings]] for the picker. You can still
  type or paste a path in the field next to it.
- A `.md` or `.markdown` file becomes a note with its text unchanged ("Markdown note"). When it has headings
  but no `#`, a ticked box "Lift headings (## → #, ### → ##, …)" moves them all up so that the highest
  becomes `#` (code and front matter stay as they are).
- A `.docx` file (Word) becomes a note too ("Word document"), when you have turned on **Settings →
  Import → Import Word files as notes** and pandoc is installed (see below). Otherwise it is saved as an
  attachment, as before; if the box is ticked but pandoc is missing, the dialog says "pandoc was not found
  (Settings → Import)".
- Imported notes keep their details (sender, date, event time, Word page setup, tags) as properties at the
  top. The reading view shows them as a card, not as raw lines (see [[Reading view]]); the editor folds
  them to one line, and an imported mail also starts with its quoted replies and signature folded (see [[Editor]]).
- Choose the folder (one per project, for example) and tags. Small pictures from signatures are
  left out by default, and each attachment can be ticked on or off. Nothing is written until you
  press Import.
- A signed mail has a signature file (`smime.p7s`, `signature.asc`), and a signature block has logos and
  social icons. They start unticked, with the reason after the name ("S/MIME signature", "signature
  picture"); tick one to bring it along. The box **Skip signatures** in the dialog (on at first; its
  start value is **Settings → Import → Skip signatures**) lets them start ticked when it is off.
  A real picture or file attached to the mail stays ticked. Keep the original message keeps every part.
- **Skip signatures** also cuts the signature **text**. A reply holds several signatures: the sender's own
  and one for each earlier mail it quotes. Shale reads the mail as parts (the own text, then each quoted
  mail: lines with `>`, "On … wrote:", "Am … schrieb …:", `-----Original Message-----`, Outlook's
  From / Sent / To / Subject block) and looks for the signature in each part: a line `-- ` (also inside
  a quote, `> -- `), the signature blocks that Thunderbird, Gmail, Outlook and Apple Mail mark in HTML mails,
  or a closing greeting ("Viele Grüße", "Best regards") followed by a few short lines with a phone number,
  address or web site, and a disclaimer at the end. The signatures go; the quoted text stays, with its
  reply header line. Shale cuts only what is clearly a signature: a mail with answers between the quoted
  lines, a `-- ` in a code block or a greeting with nothing to reach the sender below it stays as it is.
  Untick the box to keep the text as the mail had it (the editor still folds the signatures, see [[Editor]]).
- **Leave out quoted mails** (off at first; **Settings → Import → Leave out quoted mails** sets the start
  value) drops the earlier mails of the thread from the note and keeps what the sender wrote, with the
  answers between the quoted lines. A thread is often imported mail by mail.
- **A follow-up mail continues its thread.** Shale looks, by the reply headers of the mail
  (`In-Reply-To` and `References`, never the subject, the sender or the text), for notes of your vault that
  belong to the same thread. When it finds one, the mail shows **Continues** and the note's name with a tick box
  (ticked; with several notes a choice, the note the mail answers first). Ticked, the new note goes
  into the folder of that note (a folder you dropped it on or typed stays), gets `previous` and
  `next` links in the date order of the thread, and the last note of the thread gets its `next`; only
  that one line of the neighbour changes. Unticked, the note only gets its ids (`message-id`,
  `in-reply-to`, `references`, `thread-id`) and no other note is touched. A mail that is **older** than a
  note of the thread asks "Keep the thread in date order?": **Update** puts it at its place (its two
  neighbours get a new `next` / `previous` line), **Leave the chain** writes it without links. The new
  note and the neighbour lines are one entry of the file history (see [[File history]]). Renaming a
  note keeps the chain, because `previous` and `next` are links. See [[Thread view]].
  **A saved post works the same way:** when the post answers a post you saved before (or you saved
  an answer to it earlier), the Import dialog shows **Continues** for it, in the same network only,
  with the same tick box, `previous` / `next` links in time order and the question for an older post.
  **A whole thread dropped at once is chained too:** the mails (or posts) of the drop are put in date order
  together with the notes already in the vault, so each new note gets `previous` / `next` links to the others,
  whatever the order of the drop. An item that answers another item of the same drop shows **Continues:
  <its title> (in this import)**. The question for older mails is asked once per thread, on the first item
  that needs it, and the answer counts for the whole thread; **Leave the chain** then writes the thread
  without links. Notes already in the vault change only their `previous` / `next` line.
- Drop onto a folder in the file list to import into it, or onto a note to also add links to the
  new notes there.
- A picture, PDF, sound or video from outside the vault has a choice in the dialog: **Copy into the
  vault** (a copy in `z_media/`) or **Link where it is** (nothing is copied, the note links the file
  where it is). With two or more such files a line above the folder chooses for all of them. The
  choice starts as **Settings → Import → Files from outside the vault** says (Ask starts at Copy: the
  dialog is the question). A linked file keeps its name, so its title cannot be edited; the toast says
  "Linked N files where they are", and the summary lists them under "Linked where they are". See
  [[Sounds and videos]] for the links.

  ![[import-outside.png]]

- Messages that were imported before are recognised and left out unless you tick them again.
- After an import of two or more items the window **Import summary** lists what was made: each new
  note with a line about it (an e-mail: sender and date; an event: time and place; other notes: their
  first line) and an **Open** button, the files saved in the media folder, and anything that went
  wrong. **Open all in tabs** opens every new note in a tab of its own ([[Tabs]]); **Close** leaves
  them closed. One item is opened right away.

![[import-summary.png]]

![[drag-event.png]]

Each e-mail becomes a note named "date subject". The sender, recipients, date, people (links to their
contact notes), the link to the original message and the tags are in its properties (see [[Editor]]);
the text starts with the subject as a heading, then the attachments and the message. An event is
written the same way: start, end, place, calendar, repeat rule, organizer, attendees, status and
link are properties, the text is its title and its notes. Notes you imported earlier keep their
list of **From:** / **Date:** lines.

The text of a mail is tidied while it is imported: blank lines in long runs become one, a very long
tracking link becomes a short link with the host as its text, and the pictures among the attachments
are shown small (200 points wide: `![[photo.jpg|200]]`; change the number to make one bigger). The signature of the mail starts
at a line that is exactly `-- ` and earlier messages stay quoted with `>`, as in the mail.

If you read your notes in a program that does not show properties, tick **Header list in the note
body** in the Import dialog: the list of From, To, Date (event: When, Where, Attendees, …) is then also
written below the title (a saved post: Author, Posted, Source, …). **Settings → Import → Header list in the note body** sets whether the box starts
ticked.

![[imported-note.png]]

# Word files

![[settings-import.png]]

1. Install **pandoc**: Linux `sudo pacman -S pandoc-cli` or `sudo apt install pandoc`, macOS
   `brew install pandoc`, Windows `winget install JohnMacFarlane.Pandoc`, or <https://pandoc.org/installing.html>.
2. In **Settings → Import** tick "Import Word files as notes (needs pandoc)". The page shows where pandoc was
   found and its version. If it is somewhere unusual, type the path in "pandoc program"; **Check again**
   looks once more after an install.
3. Drop a `.docx` on the window. Import runs pandoc in the background (a file that takes longer than
   two minutes is stopped, and so is one for which pandoc needs more than 2 GB of memory; the message says so and
   nothing is written).

The note is made in the folder of the dialog. It keeps headings, bold, italic, underline, strike-through,
sub- and superscript, links, lists, tables (tables with merged cells become plain tables, a cell with
several lines is shown with line breaks), quotes, footnotes, math (written as `$…$` and `$$…$$`, typeset as [[Editor]] says) and highlights. A picture
becomes an embed with its caption in italics below it. A box "Lift headings" does what it does for a Markdown
file, for a document without a `#` heading.

The look of the document comes along as far as Shale can show it: the colour of text (`<span style="color: …">`; also when it comes from a character, paragraph or table
style or from a theme colour, but not the body colour, a heading's or a link's)
and of highlights, in Word's own colour and yellow too (`<mark style="background: …">`, see [[Editor]]), a title and
subtitle (a `#` heading and an italic line) and the page breaks: a manual page break, a paragraph with "page break before" and a section break that starts a new page each become a line `<div style="page-break-after: always;"></div>` where the new page begins (a break at the very start or end is left out). When Heading 1 (or Heading 2 …) starts a new page in Word, the note gets `print-break-before: 1` instead of a line before each heading. The page and the body are written into the note's properties
for the [[PDF]] export: `print-paper` (A4, A5, Letter, Legal or `<width>x<height>mm`, wider than tall is
landscape), `print-margins` (top, right, bottom, left in mm), `print-font` (serif, sans or mono),
`print-font-size` (pt), `print-line-spacing` (a factor such as 1.15), `print-paragraph-spacing` (pt after a
paragraph), `print-align` (left or justify), `print-heading-color`, `print-heading-weight` (`normal` or `bold`) and `print-heading-sizes` (pt of Heading 1 to 6), `print-break-before` (the heading level that starts a page, when Word's style says so), `print-title` (always `false`, the title
is in the text) `print-toc` (`false`, left out when the document has a table of contents) and `numbered-headings: false` (Word does not number headings unless the style does). Edit them like any
property. Page breaks come along: a manual page break, a paragraph with "page break before" and a section break that starts a new page each become a line `<div style="page-break-after: always;"></div>` where the new page begins (a break at the very start or end is left out). When Heading 1 (or Heading 2 …) starts a new page in Word, the note gets `print-break-before: 1` instead of a line before each heading. A Word table of contents (its title and the lines of entries) is not imported as text, because the
PDF export makes its own contents page. A paragraph or heading that is centred, flush right or justified in Word (in itself or by its style) while the body text is not
becomes a `<div align="center">` … `</div>` wrapper, as does a left-aligned paragraph in a justified document ([[PDF#Aligned paragraphs]]);
neighbours with the same alignment share one wrapper. Paragraphs in tables and text boxes are left as they are.

Everything one Word file brings goes into one folder, `z_media/<year>/<month>/import_docx_<name>/`, named
after the file (spaces become `-`; a number is added when the name is taken): the pictures in `media/` and, with
"Keep the original .docx" ticked, the original, which the note names in its properties as `source`. If pandoc
cannot read the file, nothing is written and a message names the file and pandoc's first error line. A file whose XML is nested
more than 256 levels deep (no real document is) is refused the same way, before pandoc starts.

# Export to Word

A note becomes a Word file again with **⋯ → Export to Word…** in the note's menu (also in the context menu of a note in the
file list), by the same **pandoc** that imports Word files; the setting "Import Word files as notes" does not have to be on.
The dialog asks where to save (`<vault>/exports/<name>.docx`, so the export shows up in the vault), whether to put a **Table of
contents** in (on or off as the [[PDF]] dialog has it for the note; Word asks to update its fields when it opens the file, say
yes to fill the contents in) and a **Page break after the table of contents** (on, so the text starts on a new page; the
note's `print-toc-page-break: false` starts it off), whether to **Open the file afterwards**, and says where pandoc was found, or that it was not
(Export then waits). The export runs in the background: closing the dialog, or **Cancel**, stops it, and a file is written only
when it is whole; if pandoc fails, nothing is written and the dialog shows pandoc's first error line (the same when it
takes more than two minutes or needs more than 2 GB of memory, which is where it is stopped). The note is exported as
the window has it, typing you have not saved yet included.

A note that was imported from a Word file, with its original kept, has the box **Use the original Word file as the template**
(ticked): the new file takes the styles, fonts, numbering, headers and footers of the original, so a Word file that goes into
Shale and comes out again looks like itself. Without it, or for a note you wrote, the look comes from the note's `print-*`
properties ([[PDF#Print properties of a note]]): the paper and margins of the page, the font, size, line and paragraph
spacing, justified text, the colour, weight and sizes of the headings and the headings that start a page. A note that says
nothing gets pandoc's own look.

What comes through: headings, bold, italic, underline, strike-through, sub- and superscript, lists, tasks (as boxes), tables,
quotes, code, footnotes, math (as Word's equations), web links, highlights (`==text==` and yellow are Word's highlight; other
background colours and text colours are kept as colours), page breaks, centred, right-aligned and justified paragraphs, and
the title block the import makes (a centred `#` heading with an italic line, as Word's *Title* and *Subtitle*). A picture is
as wide in proportion to the text as in the editor, and a picture followed by an italic paragraph has it as its caption and
as its alternative text. A link to another note is its words, and an embed of a note, a PDF or a sound is its name in
italics: Word has no place for them.

What the import already lost cannot come back: the cells that were merged in a table (they come back as empty cells),
the numbers of figures and tables (Word fields), tab stops, the line spacing of single paragraphs, small caps, and the page
setup of every section but the last. Whatever else is in the file, check it in Word; `scripts/roundtrip-docx.sh <file.docx>`
shows developers the share of the words that came back.

People and tags are found automatically: [[Tags and people]]. On Wayland Shale reads what
Thunderbird hands over; when that is only a link to the message, Shale finds the message in
Thunderbird's local copy (IMAP accounts need "Keep messages for this account on this
computer").

#userguide #import

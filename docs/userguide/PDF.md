Back to [[Start here]].

## Export

Notes and drawings export to PDF from the note menu, with no external tools: the text stays
selectable (fonts are embedded), web links stay clickable and drawings are vector. A dialog
sets paper size, margins, fonts and more. The default path is `<vault>/exports/<name>.pdf`, so
the export shows up in the vault.

A **Table of contents** (a checkbox in the dialog, on by default) puts a *Contents* page after the title: the
`#`–`###` headings, indented by level, with their numbers (see [[Notes and links]]), a dotted leader and the page
they start on; each line is a link to that page, and the text starts on the next page. The PDF also has
bookmarks for the `#`–`######` headings in your PDF reader's outline, and the headings in the text carry
their numbers. A note without such headings gets no contents page. The title (the box "The note's name as a title") is the
title line without a `YYMMDD_` date prefix; when the note's first heading is a `#` that says the same, the title is
not printed a second time.

Paragraphs are reflowed as in Markdown: the lines of a paragraph you wrapped by hand in the note are joined and laid out to the page's width. A blank line, a heading, a list, a table, a code block, an embed on its own line and a line that ends in two spaces or a backslash still end a line. Arrows, check marks and similar symbols are printed too.

Highlights (`==text==`) and the colours of [[Editor]] (`<mark style="background: …">` behind the text, `<span style="color: …">` as the text colour) are printed as written, with the tags left out and the text still selectable.

Footnotes are printed as on screen: a reference is a small raised label that links to its definition, which is printed where it stands in the note with its label raised and its text a little smaller. `<u>` is underlined, `<sub>` and `<sup>` are lowered and raised in a smaller size, and `<br>` in a table cell starts a new line in the cell (see [[Editor]]).

Math is typeset as on screen (see [[Editor]]): inline formulas sit on the baseline of their line, which is as tall as they need, display formulas are centred, and a formula in a table cell is drawn in its cell. The formulas are vector outlines of the glyphs of the math font and filled rules for bars and roots, so they stay sharp at any zoom; they are drawings, not text, so they cannot be selected or searched. A formula Shale cannot typeset is printed as code.

Code looks as on screen: fenced code blocks and inline code are printed in their syntax colours on the same dark code background as in the reading view (see [[Editor]]), the text still selectable.

Pictures, drawings and PDFs embedded with `![[…]]` on a line of their own are drawn as
thumbnails with their link as a small caption below (the same caption as in the editor, see
[[Notes and links]]); so are the first frame of a video (needs ffmpeg) and the cover of a sound.
An embed with nothing to show keeps its link text.

The size is the one you see in the note, in proportion to the text (measured against the width
of the editor's text column as it is on your screen, so a wider or narrower window gives the
same look): a picture you made half as
wide (`![[photo.png|300]]`, see [[Notes and links]]) is about as wide in the PDF in proportion to
the text, a picture that is wider than the text fills the text width, and a sound or video is
drawn as its player (cover or first frame, name, bar and length) as big as it is in the editor. The checkbox "Embedded pictures, drawings,
PDFs, videos and sounds" in the dialog switches all of this off.

Tables are exported as real tables: a bordered grid with a filled bold header row, aligned
columns and long cell text wrapping inside its column. A table that runs over a page break goes
on at the top of the next page with its header row again. Column widths and row heights you set
by dragging in the editor are used too: the widths as they are in the editor (scaled down together only when they are wider than the
page's text), the heights as least heights. Tick "Export plain text (tables as raw
text lines)" in the dialog to get the raw `| a | b |` lines instead; the box is off each time
the dialog opens, so nothing is stored in the settings.

![[pdf-export-dialog.png]]

## Print properties of a note

A note can say how it is printed, in its properties (front matter, see [[Notes and links]]). The export dialog opens with the note's
paper, margins, text size, title and contents already filled in, and you can still change them there; the keys that
the dialog has no box for (the font, the line and paragraph spacing, the alignment and the heading colour) are read from the note
when you export. Every key is optional, and a value Shale cannot read is ignored, so the default stays. Any note can carry these keys; a Word file imported with
[[Import]] gets them from the document's page setup and styles.

| Key | Values | Default | Effect |
| --- | --- | --- | --- |
| `print-paper` | `A4`, `A5`, `Letter`, `Legal`, or `<W>x<H>mm` such as `148x210mm`; a width larger than the height is landscape | `A4` | The paper size, and so its orientation. |
| `print-margins` | four numbers in mm: `top right bottom left` | `20 20 20 20` | The white space around the text. |
| `print-font` | `serif`, `sans`, `mono` | `sans` | The body font: `serif` is Liberation Serif (metric-compatible with Times New Roman), `sans` is Ubuntu Light, `mono` is Hack. |
| `print-font-size` | the body size in points, such as `12` | `11` | The size of the body text; headings and code keep their proportion to it. |
| `print-line-spacing` | a factor, such as `1.3` | built-in (1.45 of the text size) | The distance between lines; `1.0` is the font's own single line height. |
| `print-paragraph-spacing` | points, such as `8` | the built-in gap | The space after a body paragraph. |
| `print-align` | `left`, `justify` | `left` | Ragged right, or justified text. |
| `print-heading-color` | `#RRGGBB`, such as `#1F4E79` | black | The colour of the headings. |
| `print-title` | `true`, `false` | `true` | Whether the note's name is printed as a title. |
| `print-toc` | `true`, `false` | `true` | Whether a *Contents* page is printed. |

```
---
print-paper: A5
print-margins: 25 20 25 20
print-font: serif
print-font-size: 12
print-line-spacing: 1.3
print-align: justify
print-heading-color: "#1F4E79"
---
```

The properties apply to the export of a single note; a book ([[PDF#A folder as one PDF]]) uses the dialog's choices for all its notes.

## A folder as one PDF

A folder of the vault, up to the vault itself, is exported as **one PDF**, a book: right-click the
folder in the file list (or the empty space under the tree, for the whole vault) and choose
**Export to PDF…**. The PDF has a title page, one **Contents** page, and every note one after the other,
each on a new page, with a running header (the chapter's title, small and grey, top right) from the
first chapter on. The paper, margins, text size, embeds, plain tables and page numbers of the dialog apply
to the whole book; **Title page**, **Table of contents** and **Each note on a new page** are boxes of the
dialog, and the folded **Reading order** shows the tree as it will be printed (the notes left out are
greyed), so you can check it before you export. The export runs in the background, with **Cancel**.
The dialog can also write the book through a LaTeX template (below).
The default path is `<vault>/exports/<folder name>.pdf`.

Nothing has to be renamed: a folder prints as a book when it follows this convention.

```
Handbook/                     the book: its title is the folder note's `title`, else "Handbook"
  index.md                    the folder note: the introduction, the title page, the reading order
  Getting started.md          chapter 1 (index.md links it first)
  Writing/                    part 2, a folder
    Writing.md                its folder note (named like the folder)
    Lists.md                  2.1
    Tables.md                 2.2
  Appendix.md                 3 (not linked: after the linked ones, by name)
  Draft.md                    `export: false`: left out
  z_media/                    no notes: not a part (pictures come in through embeds)
```

- **Chapters are notes.** Every note of the folder and its subfolders is a chapter, except one with
  `export: false` in its properties. Pictures, drawings, PDFs, sounds and videos are no chapters: they are
  printed where a note embeds them. Folders that start with `.` or `_`, the folders `exports` and `templates`
  of the vault and folders without a chapter are left out.
- **The folder note** introduces a folder: `index.md`, else `README.md`, else the note named like the
  folder, else the one note whose properties say `folder-note: true`. Its text is printed first in the
  folder's part, under the part's heading. The top folder's note also gives the title page its `title`,
  `subtitle`, `author` and `date`.
- **The reading order** is, in a folder: the notes and folders its folder note names in `order:` (a list
  of names without `.md`), else the ones its links point to, in the order of their first link (a link
  to a subfolder's folder note stands for the subfolder), then all others by name, numbers by value (`2 Lists`
  before `10 Tables`), notes and folders mixed. A note is printed once.
- **Titles and numbers.** A chapter is called by its `title:` property, else by its title line (the file
  name without a `YYMMDD_` date prefix). Parts and chapters are numbered by their place (`2`, `2.1`),
  the headings of a note continue its chapter's number (`2.1.1`), under the setting
  **Number headings** (see [[Notes and links]]); a note's own `numbered-headings:` does not change the book.
  A `#` heading at the start of a note that says the chapter's title is not printed a second time.
- **Links between chapters** are links in the PDF: a wiki link to a note of the book (`[[Tables]]`,
  `[[Tables#Grid]]`) jumps to its page; a link to a note outside the book is its text. The contents and
  the bookmarks of your PDF reader nest parts, chapters and headings.

### A folder through a LaTeX template

The dialog of a folder has the same **Engine** choice as a note's: with **LaTeX template** the folder is
written as one LaTeX document of a template kind (see below for the templates and their setup), for a
thesis or a report kept as a folder of notes. Only kinds that hold a whole text can be chosen: the
publication, report and exercise (articles), the thesis and the PhD thesis (books); slides, letters and
posters are greyed, because they take a single note. The reading order, the folder notes and the titles are
the ones above.

- In a **thesis** each subfolder of the top folder is a `\part`, a folder inside it a `\chapter`, and the notes
  are chapters (one level below their folder); a note directly in the top folder is a chapter too, so a folder
  without subfolders is a thesis of chapters. The headings of a note go below its chapter.
- In an **article** kind the same starts at sections: a subfolder is a `\section`, its notes are
  subsections, and a note directly in the top folder is a section.
- A folder note's text is printed right after its part's heading, the top folder note's text before the
  first chapter; their headings are not numbered and not in the contents.
- The fields (title, subtitle, author, date, reviewers, …) start with the top folder note's properties, as
  a note's start with its own; the title is else the book's title.
- Links between the notes of the book go to the chapter or heading in the PDF; links to notes outside the
  book are their text. LaTeX numbers the parts, chapters and headings (under **Number headings**), makes the
  contents, the running heads and the bookmarks.
- Pictures with the same name in different folders stay two pictures.

This guide is such a book: "Start here" is its folder note, and `scripts/userguide-pdf.sh` makes its PDF
(every release has it as `shale-userguide-<version>.pdf`).
On the command line, `shale --export-pdf <folder> <file.pdf>` writes the book of a folder without opening
a window (A4, numbered headings, title page, contents, each note on a new page); it ends with 0, or with 1
and the reason on stderr.

## Read

A PDF embedded with `![[Paper.pdf]]` is drawn inside the editor and the reading view (up to 50
pages, rendered in the background). PDFs dropped on the window go through [[Import]]; see
[[Notes and links]] for embeds.

#userguide #pdf

## Export with your university's LaTeX template: the guided setup

Besides the built-in export, a note (or a folder, above) can be exported through a **LaTeX template**, for example the
corporate design of the TU Darmstadt. You do not have to know what TeX is: **Settings → LaTeX → Set up
LaTeX export…** takes you through it in five steps, and it opens by itself the first time you choose
**LaTeX template** in the export dialog of a vault where something is still missing (and from the
**Set up…** button next to each hint of that dialog: no TeX, no templates, no logo). Each step says in
plain words what it is for, shows a green tick when it is done already, and **Next** passes it; **Back**
goes a step back and **Cancel** leaves, with the export dialog or Settings back where you were.

What it costs: the built-in export needs nothing. The LaTeX export needs a program called TeX, and if
your computer has none that is new enough, Shale installs its own in a folder of its own: **about
100 MB to download and 330 MB on disk**, shown before anything is downloaded. You can remove it again in
Settings → LaTeX.

![[setup-tex.png]]

1. **TeX.** If a usable TeX is found, the step says "Found: … ✓" and you go on. If not, it says so in a
   sentence and offers **Install (about 100 MB download, 330 MB on disk)**; only that button downloads
   anything. A bar shows the progress, **Cancel install** stops it and removes what was downloaded. On
   Linux and macOS the installer needs `perl`; a computer without it is told so in words before anything is
   downloaded.
2. **Templates.** **TU Darmstadt (tested version)** is the recommended choice (the version Shale was
   tested with, its checksum is checked); **The newest version from CTAN**, **From a file or a folder**
   and **From a GitHub repository** are there for the others. A set that is in the vault already is
   offered first (**Use this set**). Importing shows its progress and can be cancelled.

   ![[setup-templates.png]]

3. **Logo.** The templates put the university's logo on the title page, and the logo may not be shipped
   with Shale or with the templates: members of the TU Darmstadt get it from the page of the university
   library about LaTeX (the same page the tuda-ci README names, linked in the step with **Open that
   page**) in the university network. Give it to Shale by **dragging the file onto the window** (a PDF or
   a PNG), or type or paste where it is. **Later** skips the step: exports then carry a grey
   placeholder and are not CI-conformant, and you can add the logo any time in Settings → LaTeX.

   ![[setup-logo.png]]

   ![[setup-logo-drop.webm]]

4. **Your details.** The **accent colour** (the colours of the templates as swatches with their codes,
   `0a` to `11d`), the **language** of your documents (German or English), your **department** and,
   if you like, your **name**. They are written to the set's `settings.toml` and are kept when the set is
   updated; the department and the name fill the fields of that name in the export dialog when a note
   gives none.

   ![[setup-details.png]]

5. **Test page.** Shale makes a small PDF from a sample note with your templates and your details
   and opens it. When it works the step says **All set. Export a note with its ⋯ menu → Export to PDF… →
   Engine: LaTeX template.**

   ![[setup-test-done.png]]

   When it does not, it shows what went
   wrong in words, **Run again** tries once more and **Save the .tex** writes the document into the vault's
   `exports` folder, to look at it or to ask someone.

   ![[setup-test-failed.png]]

The files you drop on the window while the setup is open belong to it: on the logo step a PDF or PNG
becomes the logo, on the templates step a `.zip`, a `.tar.gz` or a folder is imported as the templates; they
are never imported into the vault as notes. The same works in Settings → LaTeX: a dropped archive or
folder is imported, a dropped PDF or PNG becomes the logo of the only set that has a logo slot (or of the
set whose **Choose logo…** is open).

The setup opens by itself only once per vault (finishing or cancelling it both count); run it again from
Settings → LaTeX whenever you like. The steps are the buttons of Settings → LaTeX (below), put in order.

## LaTeX export: the TeX it needs

A note can also be exported through LaTeX templates (the first ones are the TU Darmstadt classes
`tuda-ci`; they are imported into your vault, and the export dialog uses them: see "LaTeX export:
a note through a template" below). That needs **LuaLaTeX** and **biber**. **Settings → LaTeX** sets up
the TeX and checks it:

![[settings-latex.png]]

- **Which TeX is used.** Shale looks for a `lualatex` on your computer (in `PATH`): it is accepted
  when its LaTeX kernel is from 2022-06-01 or newer (the templates need that) and `biber` is next
  to it. Shale also looks for the TeX Live it installed itself (below). The page says which one is
  used and why the other one is not, for example "the TeX of this computer misses roboto,
  xcharter; use the managed one". **Check again** looks again, for example after you installed a
  TeX Live; looking happens in the background and never stops the window.
- **TeX to use** (`tex.engine`, see [[Settings reference]]): *Automatic* takes the TeX Live that
  Shale installed if there is one, else the one of your computer if it is complete;
  the other two choices force one of them.
- **Test** compiles a small document with the fonts the templates use and a bibliography, and
  shows the result, or, when it fails, the readable problems ("package x is missing", `file:line`
  and the message of an error) and the end of the log. The first LuaLaTeX run builds a font cache
  and takes longer; the page says so.
- **Install TeX Live…** installs a TeX Live of Shale's own, with only the packages the export
  needs. Before anything is downloaded, the page tells you the sizes (**about 100 MB to download and
  330 MB on disk**), where it comes from and the folder it goes to
  (`<data folder>/shale/texlive`, on Linux `~/.local/share/shale/texlive`), and only
  **Download and install** starts it; **Not now** changes nothing. A bar shows the progress with the
  package being installed, and **Cancel install** stops it and removes what was downloaded so far.
  The folder is built under another name and renamed when it is complete, so a half-finished install is never
  used. It takes a few minutes. On Linux and macOS the installer needs `perl`; if there is none, the
  page says "install perl first" (for example `sudo apt install perl`) before downloading anything.
- **Update** brings Shale's TeX Live and its packages up to date (`tlmgr update --self --all`).
  **Remove…** asks first, naming the folder and its size, and deletes that folder and nothing else.
- **Download address** (`tex.repository`) is the TeX Live repository the install and update use;
  the default is CTAN's mirror redirector. Only `https://` (and `http://`) addresses are accepted.
  The redirector sends each request to some mirror near you, so an install or update first asks it
  once which mirror to use and then downloads everything from that one mirror. If that mirror is
  behind your installation (mirrors catch up within a day or two), the update tries another mirror,
  three times at most.

Shale runs the programs without a shell, in a temporary folder that is removed afterwards, with
each run limited to two minutes; a document cannot start programs (`\write18` is off) and cannot
write outside its folder.

## LaTeX export: template sets

The templates a LaTeX export is made with are not part of TeX Live and not part of Shale: they are
**template sets in your vault**, in the folder `templates/tex/<set>/`. A vault therefore carries
the exact class files its documents were written for; they are synced and backed up with your
notes, and an update of TeX Live cannot change a document. TeX Live supplies only the engine and
the generic packages. A set looks like this:

```
templates/tex/tuda/
  set.toml            what the set is: name, version, where it came from and when
  settings.toml       your choices: accent colour, language, department, logo (kept when you update)
  tex/                the class and package files (tudapub.cls, tudabeamer.cls, ...)
  kinds/pub.toml      one file per document kind (publication, thesis, slides, letter, ...)
  logo/               the logo you supply (kept when you update)
  README.md           a note for people browsing the vault
```

**Settings → LaTeX → Template sets** lists the sets of the open vault with their name, version,
source, number of document kinds and the state of the logo, and has these buttons:

- **Import TU Darmstadt templates** imports the corporate design of the TU Darmstadt
  ([tuda-ci](https://github.com/tudace/tuda_latex_templates), 11 kinds: publication, report,
  exercise sheet, thesis, doctoral thesis, slides, letter, poster, scientific poster, leaflet and
  announcement). It downloads the version Shale was tested with (4.06, a fixed commit whose
  checksum is checked: a different file is refused), makes the class files with docstrip using your
  TeX, and puts the finished set into the vault in one step. A progress bar shows where it is, and
  **Cancel import** stops it; a cancelled or failed import leaves the vault as it was.
- **Newest TU Darmstadt templates from CTAN** does the same with whatever version CTAN has today. It
  is not pinned and has no checksum, and the kinds were tested with 4.06 only; the page says so.
- **Import from GitHub…** takes a repository address (`https://github.com/owner/repository`) and
  optionally a branch, tag or commit. **Import from file or folder…** takes the path of a `.zip` or
  `.tar.gz` file or of a folder. Shale does not know the document kinds of such a set: it imports the
  files and says so; the kinds must be added by hand (see below). Archives are read by Shale itself
  and anything that could write outside the set's folder (`..`, absolute paths, links) refuses the
  whole archive.
- **Update set** imports the set again from where it came from. The old folder of the set is
  moved to the vault's `.trash` (so changes you made by hand to a class or a kind are not lost),
  the new files are put in place, and your `settings.toml` and your logo are kept.
- **Test page** compiles a small document of the set's publication kind (`pub`) with your TeX and
  opens the PDF, or shows the readable errors ("package x is missing", `file:line` and the message)
  and the end of the log.
- **Remove set…** asks first and then moves the set to the vault's `.trash` (never a hard delete);
  "Restore last deleted" in the file list puts it back.

### The logo

The logo of the university, `tuda_logo.pdf`, may not be redistributed, so Shale does not ship it:
members of the TU Darmstadt get it from the page of the university library about LaTeX
(<https://www.ulb.tu-darmstadt.de/finden_nutzen/medien_nutzen/dokumente_erstellen/latex/index.de.jsp>,
as the README of tuda-ci says; in the university network the file is also at
<https://download.hrz.tu-darmstadt.de/protected/ULB/tuda_logo.pdf>). **Choose logo…** takes the path of
the PDF (or a PNG) and copies it into the set's `logo/` folder under the name the classes look for;
a PDF or PNG dropped on the window while **Choose logo…** is open (or while the guided setup is on its
logo step) does the same; you can also put `tuda_logo.pdf` there yourself. Until a logo is there, the set's status says
**logo missing: exports are not CI-conformant** and every compile uses a grey placeholder bar, so
slides, letters and posters still compile; the placeholder lives only in the temporary folder of the
compile, never in your vault. Documents with the placeholder must not be used as official documents.

### Your choices: `settings.toml`

Each set has its own `settings.toml`, which a re-import keeps:

| Key | Meaning |
|---|---|
| `accent` | the accent colour of the templates; for tuda-ci `0a` to `0d` and `1a` to `11d` (default `9c`) |
| `language` | `en` or `de`: the language of the documents (class option and babel) |
| `department` | used for the `department` field of a kind when a note and the dialog give none |
| `author` | used for the `author` field of a kind when a note and the dialog give none |
| `logo` | the logo file in `logo/`, set by **Choose logo…** |

### Adding or changing a document kind: `kinds/<kind>.toml`

A kind says which LaTeX class a note is written to and which properties of the note go where. Shale
ships these files for tuda-ci and copies them into the vault; you can edit them, and for a set Shale
does not know you write them yourself (the README of the set has an example):

| Key | Meaning |
|---|---|
| `name`, `description` | what the export dialog shows |
| `mapping` | how the note becomes the document: `article` (`#` is a `\section`), `book` (`#` is a `\chapter`), `beamer` (`#` a section, `##` a frame), `letter`, `poster` (text without numbers; a class named `…sciposter` gets boxes); see below |
| `class` | the document class, a `<class>.cls` in `tex/` or in TeX Live |
| `before_class` | lines of LaTeX before `\documentclass` (for tuda-ci the PDF/A set-up that some kinds need) |
| `options` | the class options; `{accent}` and `{language}` are filled in from `settings.toml`, and an option whose `{accent}` is empty is left out |
| `preamble` | lines of LaTeX after `\documentclass` (`{language}` and `{lang}` are filled in too) |
| `logo` | `true` when the kind needs the set's logo |
| `columns` | `poster` kinds: the text of the note in this many columns (the poster has 2) |
| `[[fields]]` | one table per value: `key` (the property of the note), `label`, `command` (the LaTeX command that gets it, `title` for `\title{…}`) or `komavar` (`\setkomavar{…}` of a letter), `required`, `default` (LaTeX code used when the note has none) |

A kind that cannot be read is shown in red in the list of sets with the reason.

## LaTeX export: a note through a template

![[pdf-export-dialog-latex.png]]

Open the export dialog of a note (**⋯ → Export to PDF…**) and choose **LaTeX template** at the top, next to
**Built-in**, which stays the default and works as described under "Export" above. A drawing is
always exported with the built-in engine.

- **Template set** and **Kind** list the sets of the vault ([[PDF]], "template sets") and the kinds
  of the set: publication, report, exercise sheet, thesis, doctoral thesis, slides, letter, poster,
  scientific poster, leaflet and announcement for TU Darmstadt. A vault without a set says so and
  points to Settings → LaTeX → Template sets.
- The **fields** of the kind are text boxes, filled from the note's properties (below); a field
  with a star must have a value. The page size, margins, text size, title and page numbers are not
  in the dialog: the template decides them. **Table of contents** is there for the kinds that have
  one (publication, report, exercise sheet, thesis, doctoral thesis) and greyed out for the others;
  the box for embedded pictures stays.
- A line under the fields says which TeX is used, or **No TeX: install it in Settings → LaTeX**
  (**Export** is off then). A red warning says that the set's logo is missing: the PDF then has a grey
  placeholder where the logo goes and is not CI-conformant (see "The logo"). Next to the hints for a
  missing TeX, missing templates and a missing logo is a **Set up…** button that opens the guided setup
  (above) at that step.
- **Export** writes the note as a LaTeX document and compiles it in the background: the dialog shows
  that it is working and the window stays usable; **Cancel** (or closing the dialog) stops it. The PDF goes
  to the path of the dialog (`<vault>/exports/<name>.pdf` by default). The first export after an
  install is slower: LuaLaTeX builds a font cache.
- When it fails, the dialog stays open and shows what went wrong in words: the file and line of the
  error (`main.tex:21: Undefined control sequence.`), a package that is missing, and the end of the log.
  **Save the .tex** writes `main.tex`, its pictures and a `README.txt` with the command to compile it
  into a folder next to the PDF (`<name>-tex`), so you can look at it and run it with your own TeX.
- The fields **author** and **department** start with what you gave in the guided setup (`settings.toml`
  of the set) when the note has none.
- The dialog remembers the engine, set and kind you used last, per vault. A note can name its own
  template with the property `latex-template: tuda/thesis` (the folder of the set, a slash, the
  kind's file name); the dialog then opens on LaTeX with that kind. When the vault has no set or kind
  of that name (or the value has no `/kind`), a red line at the top says what was not found and what the
  dialog uses instead; choosing a set or kind yourself takes it away.

### How the note becomes the document

The title is the note's name without a `YYMMDD_` date prefix (or the property `title`); a first
heading `#` that says the same is not printed again. The front matter is never printed; its
properties fill the fields of the kind:

| Kind | Properties (`*` must have a value) |
|---|---|
| Publication, report | `title`* (the note's name), `subtitle`, `author`*, `date` |
| Exercise sheet | `title`*, `subtitle`, `author`*, `term`, `sheet`, `date` |
| Thesis | `title`*, `subtitle`, `author`*, `reviewer`, `department`, `institute`, `group`, `submissiondate`, `examdate` |
| Doctoral thesis | as the thesis, and `dedication` |
| Slides | `title`*, `subtitle`, `author`*, `department`, `institute`, `date` |
| Letter | `recipient`*, `subject`* (the note's name), `sender`, `sender_address`, `date`, `opening`, `closing` |
| Poster | `title`*, `subtitle`, `author`*, `footer` |
| Scientific poster | `title`*, `subtitle`, `author`*, `institute`, `footer` |
| Leaflet | `title`*, `subtitle` |
| Announcement | `title`*, `subtitle`, `author`*, `info`, `footer` |

A list (`author: [Ada, Bob]` or one `- name` per line) becomes `Ada \and Bob`; for the lines of an
address use a block:

```
---
recipient: |
  Dr. Grace Hopper
  Navy Street 1
---
```

or write `\\` between the lines in the dialog. Whatever you write in a field is text, never LaTeX: the
only exceptions are `\and` and `\\`. A field that is empty gets the default of the kind (for example the
thesis' `submissiondate` is today); `date` is not set when empty, and the class puts in today's.

**Headings** follow how they are nested, starting with `#`: in a publication, report or exercise
sheet `#` is a `\section`, `##` a `\subsection`, `###` a `\subsubsection`, deeper ones paragraphs;
in a thesis or doctoral thesis `#` is a `\chapter`, then `\section` and so on. A `###` directly under a `#`
is the next level down: no level is skipped. The numbers follow the note's property `numbered-headings` and the
setting (see [[Notes and links]]); without numbers the headings are still in the contents. The
contents list the `#` to `###` headings, and the PDF's bookmarks have all levels.

- **Slides:** `#` makes a section (for the navigation of the slides) and `##` a slide with that title and
  what is below it; smaller headings are bold lines in the slide. A note with only `#` headings has
  a slide for each. Text before the first heading, or between a `#` and its first `##`, gets a slide
  of its own. The title slide comes from the fields. A slide that is too full continues on the next
  page.
- **Letter:** the text of the note is the body of the letter; headings become bold lines. The recipient,
  subject (the note's name when you give none), opening and closing come from the properties.
- **Poster, announcement and leaflet:** the note's text with headings in the same nesting but without
  numbers; the poster sets its text in two columns. A **scientific poster** makes a box for every
  heading of the top level of the note, with its title and what is below it, in two columns.

Also exported: emphasis, strong, ~~strike-through~~, inline code, block quotes, lists, task lists
(a box, with a cross when done), rules, tables (with the alignment of their columns, a bold header and
the column widths you set in the editor), fenced code in its syntax colours on a light grey box (a long block goes on on the next page in
documents; on slides and posters it stays in one piece), web links (clickable), the highlights and colours of [[Editor]] as exact colours, and
`![[…]]` embeds on a line of their own: a picture as wide in proportion to the text as in the
note, a drawing as a vector graphic, all pages of a PDF, each with its caption; a file that is
missing is a red line `[missing: name]` in the PDF and a note in the message afterwards, and a sound
or video is such a line too. A **picture inside a line** of text (`an icon ![[icon.png]] here`, also in a
table cell) is drawn in the line, as high as the line, like a letter; a missing one is the red
`[missing: name]` in the line. Other embeds inside a line (a drawing, a PDF) stay their link text.

**Wiki links:** the PDF holds only this note. A link to one of its own headings (`[[#Results]]`,
`[[Report#Results|see the results]]`) is clickable and jumps to that heading. Links to other notes are
their text in the colour of links (red when the note does not exist), without a target, as in the
built-in export.

Also written through a template: `<u>` as underlined, `<sub>` and `<sup>` as lowered and raised text,
**footnotes** (a `[^1]` that has a definition becomes a real footnote at the bottom of the page with the
text of the definition, and the definition is not printed where it stands; a reference without a
definition stays as written; in a table cell the text comes below the table), `<br>` in a table cell as a
line break (the column then takes the room that is left, like a long text), and **math**: a formula
Shale can typeset (see [[Editor]]) is written to LaTeX as it is, `$…$` inline and `$$…$$` or a `math`
block on its own, and set by LuaLaTeX. A formula Shale cannot read is written as code, as it is, and never
reaches TeX: a note cannot run TeX commands through a formula. In a `\text{…}` a `%`, `#`, `&`, `^`, `_`,
`$` or `~` must be written with a backslash (`\%`), else the formula is refused.

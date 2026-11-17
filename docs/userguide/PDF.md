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

## Read

A PDF embedded with `![[Paper.pdf]]` is drawn inside the editor and the reading view (up to 50
pages, rendered in the background). PDFs dropped on the window go through [[Import]]; see
[[Notes and links]] for embeds.

#userguide #pdf

## LaTeX export: the TeX it needs

A note can also be exported through LaTeX templates (the first ones are the TU Darmstadt classes
`tuda-ci`; they are imported into your vault, and the export dialog that uses them comes with a
later version of Shale). That needs **LuaLaTeX** and **biber**. **Settings → LaTeX** sets up
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
get it from the corporate design portal of the TU Darmstadt. **Choose logo…** takes the path of
the PDF (or a PNG) and copies it into the set's `logo/` folder under the name the classes look for;
you can also put `tuda_logo.pdf` there yourself. Until a logo is there, the set's status says
**logo missing: exports are not CI-conformant** and every compile uses a grey placeholder bar, so
slides, letters and posters still compile; the placeholder lives only in the temporary folder of the
compile, never in your vault. Documents with the placeholder must not be used as official documents.

### Your choices: `settings.toml`

Each set has its own `settings.toml`, which a re-import keeps:

| Key | Meaning |
|---|---|
| `accent` | the accent colour of the templates; for tuda-ci `0a` to `9d` (default `9c`) |
| `language` | `en` or `de`: the language of the documents (class option and babel) |
| `department` | used for the `department` field of a kind when a note does not give one |
| `logo` | the logo file in `logo/`, set by **Choose logo…** |

### Adding or changing a document kind: `kinds/<kind>.toml`

A kind says which LaTeX class a note is written to and which properties of the note go where. Shale
ships these files for tuda-ci and copies them into the vault; you can edit them, and for a set Shale
does not know you write them yourself (the README of the set has an example):

| Key | Meaning |
|---|---|
| `name`, `description` | what the export dialog shows |
| `mapping` | how the note becomes the document: `article` (H1 title, H2 `\section`), `book` (H2 `\chapter`), `beamer` (H2 a frame), `letter`, `poster` |
| `class` | the document class, a `<class>.cls` in `tex/` or in TeX Live |
| `before_class` | lines of LaTeX before `\documentclass` (for tuda-ci the PDF/A set-up that some kinds need) |
| `options` | the class options; `{accent}` and `{language}` are filled in from `settings.toml`, and an option whose `{accent}` is empty is left out |
| `preamble` | lines of LaTeX after `\documentclass` (`{language}` and `{lang}` are filled in too) |
| `logo` | `true` when the kind needs the set's logo |
| `[[fields]]` | one table per value: `key` (the property of the note), `label`, `command` (the LaTeX command that gets it, `title` for `\title{…}`) or `komavar` (`\setkomavar{…}` of a letter), `required`, `default` (LaTeX code used when the note has none) |

A kind that cannot be read is shown in red in the list of sets with the reason. The export dialog
that lets you pick a set and a kind, and the conversion of the note itself, come with later versions of
Shale; today the sets, their logo and the test page are there.

# PDF

Back to [[Start here]].

## Export

Notes and drawings export to PDF from the note menu, with no external tools: the text stays
selectable (fonts are embedded), web links stay clickable and drawings are vector. A dialog
sets paper size, margins, fonts and more. The default path is `<vault>/exports/<name>.pdf`, so
the export shows up in the vault.

A **Table of contents** (a checkbox in the dialog, on by default) puts a *Contents* page after the title: the
`##`–`####` headings, indented by level, with their numbers (see [[Notes and links]]), a dotted leader and the page
they start on; each line is a link to that page, and the text starts on the next page. The PDF also has
bookmarks for the `##`–`######` headings in your PDF reader's outline, and the headings in the text carry
their numbers. A note without such headings gets no contents page.

Highlights (`==text==`) and the colours of [[Editor]] (`<mark style="background: …">` behind the text, `<span style="color: …">` as the text colour) are printed as written, with the tags left out and the text still selectable.

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

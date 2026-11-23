# Shale features

Shale is a notes app for the Linux, macOS and Windows desktop (what differs per system: the README, "Supported systems"). Your vault is a plain folder of markdown files,
and notes link to each other with `[[wiki links]]`. The screenshots below come from the app's
built-in demo vault (`shale --screenshot out.png`); `docs/screenshots/regen.sh` makes them all
again from the recipes in `docs/screenshots/recipes.conf`; the pictures live in the media folder of the
[user guide](userguide/Start%20here.md), a vault you can open in Shale (`docs/userguide`), which explains every feature too.

## Notes and links

![The editor](userguide/z_media/screenshots/editor.png)

- `[[Note]]` links are **blue** when the note exists and **red** when it doesn't. Click a blue
  link to open the note, or a red one to create it.
- Pictures (`![[photo.png]]`), drawings (`![[Sketch.excalidraw]]`) and PDFs are shown inside the
  editor. Pictures and videos have draggable edges and corners (the width goes into the link,
  `![[photo.png|300]]`), a round **100%** button for the natural size and a round fullscreen button.
  Every shown embed has its link as a one-line caption below it (the words of an alias,
  `![[photo.png|Sea at dusk]]`, else the file's link); a click on the caption opens the file.
  A picture embedded inside a line of text (`an icon ![[icon.png]] here`) is drawn in the line in the reading view and the
  PDF export, 0.9 of the line high and set a little below its baseline like a letter, as in the LaTeX export; the editor shows it as a picture of its own size.

![A picture with its resize buttons](userguide/z_media/screenshots/image-resize.png)

- **Syntax highlighting**: a fenced code block is coloured by the language on its opening fence
  (```` ```js ````, ```` ```rust ````, `python`, `ts`, `toml`, `sh`, `json`, `yaml`, `c++`, … about 140 languages, with the usual aliases) in the
  editor and the reading view; a block without a language or with an unknown one stays plain.
  Inline code colours its strings, numbers and called names. The PDF export draws code the same way,
  on the same dark code background (PDF-007).
- Line numbers are shown left of the text (Settings → Editor, on by default); a wrapped
  line keeps one number.
- **Folding.** Headings (up to the next heading of the same or a higher level), quotes, list items and whole lists can be
  folded away: a folded block shows its first line and a muted `… n lines` badge; the rest is hidden (no line
  numbers for it), with the Markdown preview on and off. Hover a line that starts a block and click the ▾ marker left of
  it (a folded block always shows ▸) to fold it; click the marker or the badge to unfold. Keys (Settings →
  Keybindings → Note view): **Ctrl+Shift+K** fold, **Ctrl+Shift+J** unfold, **Ctrl+Shift+O** fold all,
  **Ctrl+Shift+I** unfold all; with Vim-style editing `za` `zc` `zo` `zM` `zR`. The cursor never stays in hidden
  text: moving into a fold (or a link or search jumping there) opens it. Folds are kept per note across restarts (EDIT-079, in the vault's `.shale/state.json`; a fold whose block is gone and a note that is gone are dropped, a renamed or moved note keeps its folds) and
  are never written into the file. Also foldable (EDIT-080 to EDIT-085): **code blocks**, the **front matter** and a
  **signature** (a line `-- ` up to the next heading). A note's front matter starts folded to one muted line
  `▸ Properties · email · Anna Müller · 2026-11-12` (the setting "Properties in the editor": folded, or card, which draws the reading view's typed card there instead of the line; raw leaves it
  open); the cursor moving into it opens the YAML and folds it again when it leaves. In a **mail note**
  (`type: email`) the quotes (`… n quoted lines`) and the signature start folded too.
- **Folding in the reading view** (EDIT-095 to EDIT-099, rabalyn/shale#364): the reading view folds the same blocks with
  the same folds as the editor of the note (fold in one, it is folded in the other; kept across restarts, never written into the note).
  Hover a block's first line and click the ▾ marker left of it, or the `… n lines` badge under a folded block (a folded code block is one row with its badge in the box, EDIT-103);
  **Ctrl+Shift+O** / **Ctrl+Shift+I** fold and unfold all there (fold and unfold at the cursor are editor keys). A footnote
  jump or a click in the outline on a heading that a fold hides opens the fold. The front matter card keeps its own toggle.
  Large notes stay quick (EDIT-099 to EDIT-102, rabalyn/shale#369): the folds of an unchanged text are not found again, and
  from 2 000 lines on the reading view draws only what is near the visible part. The editor keeps the laid-out note, the
  parsed tables and footnotes and the counts of the status bar while nothing changes (EDIT-104 to EDIT-107, rabalyn/shale#372),
  so a frame of a 20 000-line note takes a few milliseconds, and the text is formatted block by block, so a typed key or a
  move of the cursor to another line formats one or two blocks instead of the whole note (EDIT-109, rabalyn/shale#375).

![Folded code blocks, quote and signature in the reading view](userguide/z_media/screenshots/reading-folds.png)
- **Code blocks as boxes** (EDIT-080, EDIT-086, rabalyn/shale#356): with the Markdown preview on, a fenced code block is a
  rounded box on the code background with a thin border and a badge at its top right with the language (`code` when there is
  none); the ``` lines are hidden and show again when the cursor is in the block. The reading view draws the same box.
  A small **copy button** (EDIT-090 to EDIT-094, rabalyn/shale#365) at its top right, left of the badge, copies the code
  without the fences and the last newline; it shows on hover (in the editor also with the cursor in the block, and on a
  folded block), in the editor and the reading view. **Ctrl+Shift+C** copies the block at the cursor.

![Code boxes, folded properties, quote and signature](userguide/z_media/screenshots/editor-code-box.png)

- The **text cursor** (Settings → Editor → Cursor) is one of four shapes: a bar before the character (default), a bar after it,
  a block or an underline; with Vim-style editing the cursor of normal mode has its own choice (default: block).
  "Blinking cursor" (on by default) makes it steady when off. The Vim tab (listed under Editor) holds the Vim settings: the switch, the cursor of normal mode, "Esc from insert mode" (the cursor stays after the typing by default, or steps back like Vim) and the link preview size. Exactly one cursor is drawn in every mode.
- A **formatting toolbar** above the text (Settings → Editor, on by default, button size 80–200 %) has a button for
  everything Shale draws: bold, italic, strikethrough, highlight, inline code, headings (a menu from
  Normal text to H6), bullet and numbered lists, tasks, quotes, code blocks, rules, page breaks, alignment (left, centre, right), tables, `[[note]]`
  and `[text](url)` links, embeds (it opens the `[!` file picker) and "Clear formatting". With a
  selection a button wraps it (or takes the markers off again); with none it puts the markers in and the
  cursor between them, and the same key again just before the closing marker steps out behind it (an empty
  pair is taken off); a line button works on every line the selection touches. Each click is one undo
  step and the keyboard stays in the note, also with the Markdown preview and with vim-style editing.
  Every button of the toolbar, the view header (edit / reading view, preview, ⋯ menu), the tab bar (New tab)
  and the ribbon has an Alt key (Alt+B bold, Alt+V reading view, Alt+1 … Alt+0 ribbon items, Alt+, and Alt+.
  sidebars, see Keyboard below), shown as a badge at the button while Alt is held; after a button or key that
  only changes the view, the editor has the keyboard again with the cursor and selection unchanged.
- **Colours.** The toolbar's highlight and font colour buttons are split buttons: a click on the colour applies the
  current colour at once (the colours are remembered in the vault, also after a restart), the ▾ beside it opens a palette (four neon highlights, a
  "No highlight" entry and a colour picker; font colours you set in Settings → Editor; Alt+Y / Alt+F apply the current colour,
  Alt+Shift+Y / Alt+Shift+F open the palette, where the digits 1 to 8 pick a swatch and 0 picks none) and write plain
  HTML that other markdown apps understand: `<mark style="background: #FFFF00;">text</mark>` and
  `<span style="color: #E53E3E;">text</span>`. They show in the editor (the tags dimmed, or hidden by
  the Markdown preview), the reading view, table cells and the PDF export, and nest with bold and italic.
- **Footnotes, underline, sub and superscript.** A footnote reference `[^1]` with a definition
  `[^1]: text` (continuation lines indented by four spaces or a tab) is a small raised label in the
  link colour, the definition shows its label raised and its text a little smaller; in the reading
  view a click on a reference jumps to its definition and back. `<u>text</u>` underlines,
  `<sub>…</sub>` lowers and `<sup>…</sup>` raises, and `<br>` in a table cell starts a new line.
  All of it shows in the Markdown preview, the reading view, the outline, table cells and the PDF
  export (a reference links to its definition) and is what pandoc writes for a Word document.

  ![Colours and the toolbar](userguide/z_media/screenshots/editor-colours.png)

- **Math.** `$E = mc^2$` is set in the text, and `$$…$$` on lines of its own (or a ```` ```math ```` block) is
  set centred in display style: fractions, roots, scripts, sums and integrals with limits, matrices,
  cases, delimiters that grow and the usual symbols, typeset by Shale itself (pure Rust, no TeX needed)
  with the bundled font STIX Two Math. It shows in the reading view, the editor's Markdown preview (the
  cursor line shows its TeX), table cells, the outline and the PDF export, where it is drawn as sharp
  vector outlines; a formula Shale cannot typeset stays as code, and says why when you point at it.

  ![Math in the reading view](userguide/z_media/screenshots/math-reading.png)
  ![Math in the Markdown preview](userguide/z_media/screenshots/math-editor.png)

- Lists continue when you press Enter (also numbered lists and to-dos `- [ ]` / `1. [ ]`).
  Tab and Shift+Tab indent list items or change heading levels, and Alt+↑/↓ move lines.
- Web links (`[text](https://…)`) open in your browser.
- Drag notes, drawings, files and folders in the file list to move them into another folder.
  Links that point to them by path are updated.

![Moving a note into a folder](userguide/z_media/screenshots/tree-drag.png)

- Back and forward with the mouse side buttons or Alt+←/→ (macOS: Cmd+[ / Cmd+]). You can sort the file list by name,
  date modified or date created.

![Suggestions while typing](userguide/z_media/screenshots/link-suggestions.png)

- Type `[[` to get matching notes; ↑/↓ or Ctrl+J/Ctrl+K move through the list, Enter, Tab or a
  click picks an entry, and the last entry creates a new note. Esc closes the list.
- Type `#` and a letter (or a word in the `tags:` property at the top of a note) to get the vault's
  tags that match, best match first: `#prj` finds `projects/shale`, nested tags included; ↑/↓ or
  Ctrl+J/Ctrl+K choose, Enter, Tab or a click picks, Esc closes. Nothing opens in code, in a link, in
  a URL or on a heading (EDIT-120).
- Type `[!` to pick a file: `/` starts at the vault, `~/` at your home folder, and Tab completes
  like a shell; ↑/↓ or Ctrl+J/Ctrl+K choose, Enter or a click picks. A file from outside the vault is
  copied to `z_media/<year>/<month>/`, or (a picture, PDF, sound or video) linked where it is, or
  asked about, as Settings → Import → Files from outside the vault says; the last line of the picker
  shows what a pick does and **Alt+Shift+L** changes it for this pick (MEDIA-019).
- Paste a picture (Ctrl+V, or `p` in vim mode) and it is saved in the media folder as
  `Pasted image.png` with a link at the cursor; paste files copied in a file manager (pictures, PDFs,
  audio, video) and they are copied there and linked, one link per line, or linked where they are,
  or a small question asks which (the same setting). Other text pastes as usual.

- **Sound players.** `![[song.mp3]]` shows a small player instead of a link, in the editor, the
  markdown preview and the reading view (mp3, ogg, oga, flac, m4a, aac and wav play, also the A-law and µ-law WAV files of phone systems
  and answering machines such as FRITZ!Box call recordings; what Shale cannot decode itself, opus and
  ADPCM WAV among it, plays through ffmpeg when that is installed, and otherwise the player says what
  the file holds instead of calling it damaged). It shows the cover from the file's tags (or a tile
  with the file name), a round play/pause button over it (in the reading view Space too, once a click gave the player
  the keyboard; in the editor a click on the player leaves the keyboard and the cursor with the text), a progress bar (click to jump, drag to choose and seek on release) with the time,
  and a mute button and volume slider. Only one file plays at a time; it keeps playing while
  you scroll or switch between the editor, the preview and the reading view, and stops when
  the note's tab is closed or another vault is opened. Nothing ever starts by itself. In Settings →
  Appearance: how opaque the play button is (60 % by default). The volume is one setting for
  all players.

![A sound player in a note](userguide/z_media/screenshots/sound-player.png)

- **Video players.** `![[clip.mp4]]` (mp4, m4v, mov, webm, mkv, avi, ogv) shows the same player with the
  first frame as its picture; the picture is wider (16:9) and the controls sit below it. While it
  plays the picture shows the video, and the sound goes through the same output as sound files.
  A click on the progress bar restarts the video at that place; a click before the length is
  known waits for it and then jumps. Videos are decoded by the
  **ffmpeg on your computer** (`ffmpeg` and `ffprobe`; `apt install ffmpeg`, `brew install ffmpeg`,
  `winget install ffmpeg`). Without it the video shows a tile with a play mark, a click opens the
  file in your system's video player, and Settings → Appearance says that ffmpeg is missing and
  where to get it.

- **Media outside the vault** (ADR-0030). A picture, PDF, sound or video can stay where it is. A
  **library link** `![[@Videos/2024/a.mp4]]` names a file in the library folder `Videos`, which each
  computer maps to a local folder in its own settings (`media_libraries` in the settings file); a vault
  file at that path wins, and the rest of the path may not leave the folder. A **file link**
  `![](<file:///home/ada/Videos/a.mp4>)` (also `%20`, and `file:///C:/…`) is Obsidian's form. Both show
  like media of the vault, in the editor, the preview, the reading view, the link preview and the PDF
  export. A missing file, or a library that is not set up here, says so in place of the embed. A click
  opens the file with your system's program and never makes a note; such links are not red, not in the
  graph and not offered by "Fix link". Only these four kinds of file are opened this way, and nothing
  outside the vault is ever written. Outside files are not synced or kept in the file history
  (MEDIA-023): Settings → Sync says how many files the vault links from outside, and the caption of such
  a file says "Outside the vault: not synced" on hover.
  **Settings → Import → Files from outside the vault** says what a pick with `[!`, a paste or an import
  does with a picture, PDF, sound or video that is not in the vault: **Ask** (the start value), **Copy
  into the vault** or **Link where they are** (MEDIA-019); the library folders of this computer (a name
  and a folder each) are set on the same page. A linked file under a library folder is written
  `![[@Videos/2024/a.mp4]]`, any other `![](<file:///…>)` (MEDIA-020). Nothing is linked because it lies
  in a library folder: every entry point follows the setting. An outside embed that is there has **Copy
  into the vault** (the right end of its caption in the editor and the preview, a button or a line in the
  reading view), and the note's ⋯ menu has **Copy outside media into the vault** for all of its links: the
  files go to `z_media/` in the background and the links are rewritten (width and alias kept, one undo step,
  MEDIA-021). **Settings → Vault → Check names for Windows and macOS…** also lists the vault's outside
  links by note (a file that is not found, a library not set up on this computer or whose folder is not
  there, file links that work on this computer only; MEDIA-022), and **Data usage** counts the outside
  files the vault links and their size (USE-026), both in background jobs. The window never waits for
  the disk of a shown picture or media file: whether a file is there, its date and its decoding are looked
  up in the background and asked again every few seconds, and an embed says "Loading …" until the answer
  is there, so a NAS or USB disk that does not answer freezes nothing (MEDIA-024). The Import tab of Settings is
  shown under "Importing e-mails, events and files".

![Reading view](userguide/z_media/screenshots/reading.png)

- Ctrl+E switches between editing and the reading view.

![The note menu](userguide/z_media/screenshots/more-menu.png)

- Export notes and drawings to PDF (paper size, margins, fonts and more in a dialog; tables become
  real grids with the header repeated across pages, or raw text lines with "Export plain text";
  embedded pictures, drawings, PDFs, the first frame of a video and the cover of a sound are
  drawn as thumbnails with their link as the caption below, as big in proportion to the text as in
  the editor: a picture made smaller keeps its share of the width, a sound or video is drawn as its
  player card; a picture inside a line of text is drawn in the line, as high as the text; highlights and font colours are printed as written, as are footnotes (a reference links to
  its definition), underline, sub and superscript, `<br>` in table cells and math (vector outlines); a **Contents** page after the title lists the
  `#`–`###` headings with their numbers and the pages they start on, each a link, with a checkbox to leave it
  out, and the PDF has bookmarks for its `#`–`######` headings; the title is printed once: when the note's first
  heading is a `#` that says the same as the title, the separate title is left out, PDF-008; a note's
  properties set how it is printed, see below), insert a
  new drawing, rename the note (links in other notes are updated: wikilinks, embeds of notes and files, Markdown links and links in properties, also when a file or folder is moved or renamed from the title line; one history step), or move it to the trash. The
  trash keeps the original folder (`.trash/<date>/<path>`) and nothing in it is replaced or
  emptied; right-click the empty space of the file list and choose "Restore last deleted" to put
  the last item you trashed back.

  ![The PDF export dialog](userguide/z_media/screenshots/pdf-export-dialog.png)

- **Print properties of a note** (#339): the front matter of a note can say how its PDF is printed; all
  keys are optional and a bad value is ignored. The export dialog opens with the note's paper, margins,
  text size, title and contents filled in (and they can be changed there); the font, the spacing, the
  alignment, the heading colour, weight and sizes are read from the note at export time. Word imports write them.
  - `print-paper`: `A4`, `A5`, `Letter`, `Legal` or `<W>x<H>mm` (a width larger than the height is landscape)
  - `print-margins`: four numbers in mm, `top right bottom left`
  - `print-font`: `serif` (Liberation Serif, Times-compatible), `sans` (Ubuntu Light) or `mono` (Hack)
  - `print-font-size`: the body text in points
  - `print-line-spacing`: a factor, 1.0 being the font's own single line height
  - `print-paragraph-spacing`: the space after a body paragraph in points
  - `print-align`: `left` or `justify`
  - `print-heading-color`: `#RRGGBB`
  - `print-heading-weight`: `normal` or `bold` (the default), the weight of headings
  - `print-heading-sizes`: six sizes in pt for the heading levels 1 to 6, such as `"16 13 12 11 11 11"` (4 to 72
    each; anything else is ignored); they replace the usual proportions
  - `print-title`: `true` or `false`, the note's name as a title
  - `print-toc`: `true` or `false`, the contents page
  - `print-toc-page-break` (#578, PDF-031): `true` (the default) or `false`; with `true` the text after the contents starts a new page
    (the dialogs of the PDF, LaTeX, Word and folder-book exports have the box "Page break after the table of contents")
  - `print-break-before`: a heading level 1 to 6; every heading up to that level starts a new page
    (`1`: every `#`; the dialog has a box "Start each # heading on a new page" for it)
  - **Paper look** (#367): a note with any of these keys is printed plainly: list marks and quotes in the text
    colour (a quote indented, without a bar), inline code and code blocks in the mono font without the app's
    colours (a code block on light grey), tables with thin black lines and a bold header without fill. A
    note without any of the keys prints as before.

- **Page breaks** (#359): a line `<div style="page-break-after: always;"></div>` (the form Obsidian writes;
  `page-break-before: always`, `break-after: page` and `break-before: page` work too) is a page break:
  the built-in PDF export and the LaTeX export (`\newpage`) start a new page there, without an empty
  page when the page is empty already or the break is the last thing of the note. The editor's Markdown
  preview and the reading view draw it as a thin dashed rule labelled "Page break" (the line with the
  cursor shows the HTML). The toolbar button "Page break" (Alt+J) inserts the line.

- **Centred, right-aligned and justified paragraphs** (#340): a paragraph or heading between a line
  `<div align="center">` and a line `</div>` (also `align="right"` and `align="justify"`; the form Obsidian
  shows too) is aligned that way. The reading view, the built-in PDF export and the LaTeX export
  (`center` and `flushright`) lay it out so, and the outline still lists a heading in one; the editor shows
  the two lines dimmed. Three toolbar buttons (left, centre, right) put the paragraphs under the cursor or
  the selection into a wrapper, change it or take it off. The Word import writes the wrappers for the
  paragraphs and headings whose alignment differs from the body's.

- **A folder as one PDF, a book** (BOOK, #306): **Export to PDF…** in the menu of a folder in the file list,
  or of the vault (right-click the empty space under the tree), writes the notes of the folder and its
  subfolders as one PDF: a title page (the folder note's `title`, `subtitle`, `author`, `date`, and a `logo:` picture above the title, BOOK-019), one
  contents page (parts, chapters and the first level of headings, with numbers, a dotted leader and the
  page, each a link), every note on a new page (a box), numbered headings that continue the chapter's number,
  a small grey running header with the chapter's title, links between the chapters that go to the page
  (or the heading), and nested bookmarks. The convention needs no renaming: chapters are the notes (not
  `export: false`), a folder note (`index.md`, `README.md`, the note named like the folder, or
  `folder-note: true`) introduces a folder, the reading order is its `order:` list, else the order of its
  links, then the rest by name with numbers by value. The dialog has a folded **Reading order** to check the
  tree; the export is a background job with Cancel.
  `shale --export-pdf <folder> <file.pdf>` does it without a window, and `scripts/userguide-pdf.sh` makes the PDF of this
  user guide (BOOK-001 to BOOK-012, BOOK-019), which every release has as `shale-userguide-<version>.pdf`.
- **A folder through a LaTeX template** (#309): the folder's export dialog has the engine choice of a note's.
  With **LaTeX template** and a kind of the article or book family (publication, report, exercise, thesis,
  PhD; slides, letters and posters are greyed) the book is one LaTeX document: in a thesis the subfolders are
  `\part`s and the notes `\chapter`s, in an article kind they start at `\section`; folder notes' text goes
  under their part, the top folder note's before the first chapter and its properties fill the fields; links
  between the chapters jump to their chapter or heading (`\hyperlink`); embeds of the same name in different folders stay
  apart. LaTeX makes the numbers, the contents and the bookmarks (BOOK-013 to BOOK-017).

- **Settings → LaTeX** finds the TeX for the LaTeX export of notes (LuaLaTeX and biber, the one on the
  computer when its kernel is from 2022-06-01 or newer, else a TeX Live that Shale installs itself):
  the page says which TeX is used or why none can be, tests it with a small document, and installs,
  updates and removes the managed TeX Live. It shows the sizes (about 100 MB to download, 330 MB on
  disk) before anything is downloaded, shows the progress, and can be cancelled; the compile that
  the export will use runs LuaLaTeX and biber in a temporary folder with a timeout and turns the log
  into readable errors (TEX-001 to TEX-004). After an install or update the font list of TeX is built once, a font that cannot be loaded is repaired by installing the font packages again from another mirror, and the result names the mirror (#362).

  ![The LaTeX tab of Settings with the question before the install](userguide/z_media/screenshots/settings-latex.png)

- **Template sets** (Settings → LaTeX) are the templates a LaTeX export is made with, kept in your vault
  (`templates/tex/<set>/`: the class files, one small manifest per document kind, your settings and the
  logo), so they are synced and backed up with your notes and a TeX Live update cannot change a document.
  **Import TU Darmstadt templates** downloads the tested tuda-ci (checked by its checksum) and builds the
  11 document kinds (publication, report, exercise sheet, thesis, doctoral thesis, slides, letter,
  poster, scientific poster, leaflet, announcement); the newest version from CTAN, a GitHub repository, a
  zip, a `.tar.gz` or a folder can be imported too (a set Shale has no kinds for is imported without
  kinds, which are then added by hand). Importing runs as a job with progress and Cancel and never leaves
  half a set; **Update set** keeps your settings and logo and moves the old files to the vault's trash;
  **Test page** compiles a small document of the set and opens the PDF; **Remove set…** moves it to the
  trash. The university's logo may not be redistributed: **Choose logo…** copies yours into the set, and
  until then exports use a placeholder and the set says "logo missing: exports are not CI-conformant"
  (TEX-005 to TEX-007, TEX-024 to TEX-027). A **department logo** (**Choose department logo…**, **Remove
  department logo**) is an optional second slot: documents show it below the TU logo, right-aligned and
  as high, where the class has room for it (not on exercise sheets and announcements), and the export
  dialog has a **Department logo** box for it (TEX-035).

  ![Template sets in Settings → LaTeX](userguide/z_media/screenshots/settings-latex-sets.png)

- **Guided setup of the LaTeX export** (Settings → LaTeX → **Set up LaTeX export…**, and by itself the first
  time you choose **LaTeX template** in the export dialog of a vault where something is missing; the dialog's
  hints for a missing TeX, template set or logo have a **Set up…** button too) is for people who are not firm in
  TeX but want to use their university's templates. Five steps with a step indicator, Back, Next and Cancel,
  and a green tick on what is done already: **TeX** (found, or installed after you agree: about 100 MB to
  download and 330 MB on disk, with progress and Cancel; the built-in export needs nothing of this),
  **Templates** (TU Darmstadt, the tested version, the newest from CTAN, a file, a folder or GitHub, or a set
  you have), **Logo** (the university's logo may not be shipped: the step says where members get it; give it
  by **dropping the file onto the window** or by its path, or choose Later and exports carry a placeholder; an
  optional second field takes your department's logo),
  **Your details** (the accent colour as swatches `0a` to `11d`, the language, your department and name, kept in
  the set's `settings.toml`) and a **Test page** that is made with your choices and opened, or, if it fails,
  explained in words with **Save the .tex**. While the setup or Settings → LaTeX is open, files dropped on the
  window belong to it (a logo, a `.zip`, a `.tar.gz` or a folder of templates) and are not imported into the
  vault as notes (TEX-017, TEX-028).

  ![Step 1: TeX](userguide/z_media/screenshots/setup-tex.png)

  ![Step 2: the templates](userguide/z_media/screenshots/setup-templates.png)

  ![Step 3: the logo](userguide/z_media/screenshots/setup-logo.png)

  ![Step 4: your details](userguide/z_media/screenshots/setup-details.png)

  ![Step 5: the test page, when it worked](userguide/z_media/screenshots/setup-test-done.png)

  ![Step 5: the test page, when it did not work](userguide/z_media/screenshots/setup-test-failed.png)

- **Export through a LaTeX template**: the PDF dialog of a note has an engine choice, **Built-in** (the
  default) or **LaTeX template**. With the template you pick a set of the vault and a kind (publication,
  report, exercise sheet, thesis, doctoral thesis, slides, letter, poster, scientific poster, leaflet,
  announcement), fill the kind's fields (taken from the note's properties, `author:`, `subtitle:`,
  `recipient:` and so on), switch the table of contents, and Export compiles the note with LuaLaTeX in
  the background (a spinner, Cancel). Headings become sections, chapters, slides or poster boxes by how
  they are nested; emphasis, code in colours, lists, tasks, tables with their column sizes, quotes,
  links, colours, pictures, drawings (as vector graphics) and PDF pages are written, with every special
  character kept as text; code sits on a light box, a picture inside a line is as high as the line, and a
  link to a heading of the note jumps there in the PDF (links to other notes are link-coloured text). A failure shows the file and line of the error or the package that is missing,
  and **Save the .tex** writes the document and its pictures into a folder for debugging with your own
  TeX. The dialog remembers the last engine, set and kind per vault, and a note can name its template
  with `latex-template: tuda/thesis`; a template the vault does not have, or a kind a folder's book cannot use, is said in the dialog (TEX-008 to
  TEX-016, TEX-018, TEX-019, TEX-030 to TEX-033).

  ![The export dialog with the LaTeX template engine](userguide/z_media/screenshots/pdf-export-dialog-latex.png)

- Every dialog is a window you can move by its title bar and resize from any edge or corner.

## Tabs

- Drag a tab left or right to reorder it; a marker shows where it will land. A click without
  moving still just selects the tab, and the ✕ or a middle click closes it.
- Right-click a tab and choose **Pin tab** (or **Unpin tab**): pinned tabs move to the far left,
  show a small pin and keep their order. Drag reorders pinned tabs among themselves and the
  others among themselves, never across the border. A pinned tab can still be closed. A pinned tab is
  never replaced: opening a note any way (an import, a link, a search result, the quick switcher, the
  file tree, back and forward) opens it in a new tab instead.
- The order of all open tabs (notes, drawings and Feeds tabs) and which of them are pinned come
  back at the next start.

![A pinned tab and its context menu](userguide/z_media/screenshots/tab-pinned-menu.png)

## Panes side by side

- The middle area splits into any number of **panes** side by side, each with its own tab bar,
  tabs, open tab and back / forward history. **Ctrl+\** (or **Open in split right** in the
  right-click menu of a tab) opens the tab in a new pane to the right of the focused one.
- One pane has the focus: click into it (tab bar or content) or press **F6 / Shift+F6**.
  Its tab bar has an accent line. Opening notes and links, the quick switcher, new notes, closing
  and switching tabs, back / forward and the reading view all work on the focused pane; renaming,
  moving or deleting a file updates its tabs in every pane.
- Drag the border between two panes to resize them (neither gets narrower than 240 points).
  Closing the last tab of a pane (or **Ctrl+Shift+W**) removes the pane; the last pane stays.
- The panes, their tabs, the open tab of each, the focused pane and the widths come back at the
  next start, and each note with its cursor or selection, its scroll position and its reading view. Mail,
  Calendar, Tasks, Graph and Settings-file tabs come back too: the mail view on its folder and open message,
  the calendar on its mode and day, Tasks with its filter, the graph with its switches (an empty tab is
  not kept); the
  state is written every few seconds when something changed, and at exit. All the keys can be changed in Settings, Keybindings.

## Fonts and text sizes

- Settings → Fonts sets the font and size of six areas: dialogs and menus, the raw markdown text of
  the editor, the rendered text (markdown preview and reading view), mail, calendar and feeds.
  The raw and the rendered text of the editor can have different fonts and sizes.
- "Use the same font and size everywhere" (on by default) shows one row that applies to all areas;
  switched off, every area has a row of its own, starting from what it had.
- A font is chosen from a list of the fonts installed on the computer: a button opens a list with a
  search field (type to narrow it, ↑ ↓ and Enter to pick, Esc to close); "Default" is Shale's own
  font. Every row shows a sample text in its font and size, so all chosen fonts can be compared at
  once; while the list is open, the sample shows the font under the mouse or the arrow keys
  without picking it. Characters a chosen font lacks (emoji) are drawn with Shale's own fonts. Bold and italic text
  is drawn in the font's own bold, italic and bold-italic faces where it has them. A font that is no
  longer installed is replaced by Shale's own, with a message, and used again when it comes back.
- Changes show at once, also when you edit `[fonts]` in the settings file. The old "Text size"
  setting became the size of the editor.

![The Fonts tab of Settings, one row per area](userguide/z_media/screenshots/settings-fonts.png)

## Themes

- Settings → Appearance → Theme sets the colours of the whole window. Nine themes come with Shale: the dark
  **Shale** (the default, as the app always looked), **Dracula**, **Nord**, **Catppuccin Mocha** and
  **Gruvbox Dark**, and the light **Catppuccin Latte**, **Solarized Light**, **GitHub Light** and
  **Gruvbox Light**. A change shows at once, also when you edit `[theme]` in the settings file.
- "Follow the system's light or dark mode" swaps the one list for two, a light theme and a dark theme,
  and Shale uses the one that matches the system (the dark one when the mode is not known). The
  settings are `[theme]` `name`, `follow_system`, `light` and `dark`.
- A theme is a terminal colour scheme in the Alacritty TOML format, read as it is: put a `.toml` file
  of the alacritty-theme collection (or your own) into the themes folder next to the settings file
  ("Open themes folder"; "Reload themes") and it is listed by its file name. A file named like a
  built-in theme replaces it. A file that cannot be read is skipped and the reason is listed.
  Shale makes its greys, accent, links, code and status colours from the terminal colours and
  keeps them readable (a contrast of 3 to 1, text 4.5 to 1); an optional `[shale]` table in the
  file sets any colour of the window exactly (CFG-062 to CFG-065).
- Colours set in a note (`<mark>`, `<span>`) are kept as written, the drawing canvas keeps its own
  switch and the PDF export prints with Shale's colours. The bundled themes' authors and licences
  are in `assets/themes/LICENSES.md`.

![The Appearance tab of Settings with the theme list open](userguide/z_media/screenshots/settings-appearance.png)

![The window in the light theme Catppuccin Latte](userguide/z_media/screenshots/theme-latte.png)

## Keyboard only: sidebar, file list and vim-style editing

![The Keybindings tab of the settings](userguide/z_media/screenshots/keybindings.png)

- Ctrl+H (macOS: Cmd+Shift+H) puts the keyboard one place to the left (with split panes: into the pane on the left,
  from the first pane into the left sidebar), Ctrl+L one place to the right (from the sidebar into
  the first pane, then pane by pane; it stays in the last one). Ctrl+Shift+B opens
  the file list with the open note selected: j / k go from file to file, Ctrl+J / Ctrl+K from
  folder to folder, Space opens or closes a folder, Enter opens a file.
- The Keybindings tab has a filter field (Ctrl+F or `/` focuses it): it keeps the actions whose name or
  keys fit what you type, such as `ctrl+h` or `F6`, hides the groups with no match and says "No
  action matches" when nothing does. Change, Add, Clear and Reset work on the rows shown; Reset all still
  resets every action.

  ![The Keybindings tab filtered by ctrl+h](userguide/z_media/screenshots/keybindings-filter.png)
- Every shortcut of this section can be changed in Settings → Keybindings; a key that another
  action already uses asks “Overwrite it?” (Yes takes it from that action and leaves it without a key, No
  changes nothing; Enter answers No), and a binding can be a sequence of keys. While you record, a note
  under the keys says which action they would clash with.
- Ctrl+PageUp / Ctrl+PageDown and Ctrl+Shift+Tab / Ctrl+Tab (macOS: Cmd+Opt+← / Cmd+Opt+→) switch to the previous / next tab of the focused pane, also from the file list and from
  mail, calendar and Feeds; after the last tab comes the first.
- In the search results j / k go from hit to hit and Ctrl+J / Ctrl+K from file to file (in the mail and posts groups from group to group).
- **Markdown preview** (Settings → Editor, or the eye icon in the top right of the editor;
  off by default): every line is drawn rendered, with headings at their size, bold, italic,
  strike and code without their markers, links as just their text, and only the line the cursor
  is in (and, with "Raw lines around the cursor", that many lines before and after, or all the
  lines of a selection) shows the raw markdown. The note stays editable; links still open with a
  click and task boxes still tick (a task away from the cursor is drawn with a painted checkbox in place of its
  `- [ ]`, and a list item with a dot, filled and hollow by level, in place of its `-`, `*` or `+`, EDIT-016). A **table** away from the cursor is drawn as a real grid (also
  in the reading view): borders, a bold header row, aligned columns, formatted cell text, long
  text wrapping in its own column. Click a cell to edit it right in the grid (Tab, Enter and
  their Shift versions move between cells, so do the arrow keys at the edge of a cell's text;
  Esc leaves; one undo step), or turn that off in
  Settings ("Edit table cells in the grid") or per table (a button at the grid) to edit the raw
  rows instead. Drag the borders to resize columns and rows, the edges to resize the whole table;
  a double click on a border makes all columns (rows) the same size. Hover the grid for buttons
  that add a row or column and delete the hovered row or column. Toolbar formatting of a
  selection across cells formats each cell on its own; while a cell is edited in the grid the
  toolbar formats the cell's text (the line buttons are greyed out) and the change is part of
  the cell's undo step. **Shift+arrow keys** select a rectangular block of cells (Shift+Right then
  Shift+Up gives 2 by 2; the arrows select text first and go on to cells at the edge of the text):
  the selected cells are highlighted, the toolbar's inline buttons format all of them, Delete and
  Backspace clear them (one undo step each editing), typing goes into the block's top left cell,
  and a plain arrow key or Esc ends the block.

  ![The Markdown preview](userguide/z_media/screenshots/editor-preview.png)

  ![A table as a grid, with its buttons](userguide/z_media/screenshots/table-grid.png)

  ![A table cell edited in the grid](userguide/z_media/screenshots/table-cell-edit.png)

  ![A block of 2 by 2 cells selected with Shift+arrow keys](userguide/z_media/screenshots/table-cell-block.png)

  ![A table in the reading view](userguide/z_media/screenshots/table-reading.png)

- **Vim-style editing** (Settings → Vim, off by default): notes open in normal mode. h j k l, w b,
  W B, 0 $, gg G move; `:12` jumps to line 12; `/text` searches and n / N go on; i a I A o O
  start typing, Esc stops; x deletes a character; `dd` deletes the line (onto the clipboard,
  `p` puts it back below); v and V select with the same motions, d cuts and y copies to the
  clipboard, p pastes; `gt` follows the link under the cursor like a click (a note link or a web
  address); **Shift+K** (definable, for example as Space Space) previews the link under the cursor in a floating frame like neovim's hover: a note as the reading view draws it (from the heading of the link), "Note does not exist yet" for a missing one, a picture, or for a web link the page's title, description and the start of its text, loaded in the background (https only, 30 s, 1.5 MB, kept per address while Shale runs). Moving the cursor, typing or Esc close it; Shift+K again moves the keyboard into it (j / k, ↓ / ↑, Ctrl+D / Ctrl+U, PageDown / PageUp scroll, Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc gives the keyboard back to the editor with the cursor where it was). The frame is 640 x 480 points at first; its four edges and corners are dragged to resize it, and the size is a setting (`link_preview_width`, `link_preview_height`, Settings → Editor, with Reset size) that every preview uses (EDIT-037). `u` undoes and Ctrl+R redoes, up to 200 steps per note (an insert session
  from i to Esc is one step; the history is shared by all tabs of the note and kept in the vault
  for 30 days by default (a setting of the undo histories cache, see Data usage), so `u` works
  after a restart too). The status bar shows NORMAL, INSERT, VISUAL or V-LINE.
- In normal mode (and in a calendar, Feeds, … tab) **Space Tab** goes back to the note or sub-app
  this tab showed just before, and again to come back, like vim's alternate file. Space is the
  leader key of its own list in the Keybindings tab; **Space f f** (and Space f g) work in every main view, not only in normal mode: the editor in normal mode, the reading view, mail, calendar, Feeds, tasks, graph; not while a text field has the keyboard, under a dialog or in a drawing (scope "Main views" of the Keybindings tab; in the mail view the mark of a message is **x**, Space is the leader); **Space f f** opens a window to find a file by its
  name (it never creates a note, unlike Ctrl+O); **Space f g** searches the text of notes, mail
  and posts in one list grouped by source (`*` and `?` are wildcards); the vim keys themselves cannot be rebound.
- **Choose the sources of the search** (the sidebar search, Ctrl+Shift+F, and `Space f g`): a prefix at the start of
  the query limits it (`f:` notes, `m:` mail, `s:` posts, also combined: `fm:`), and a button per source with its
  count (**Notes 12 · Mail 20+ · Posts 4**, a switched-off source is counted, in grey) lists only that source on a
  click (again: back), or goes in and out of the default selection on Shift+click. Tab / Shift+Tab step through the
  sources. The default selection is saved; the letters are in Settings → Search.
- **Search syntax** (user guide: *Search syntax*): the sources read the same text differently. **Notes** look for the whole
  query as one piece of text inside a line, ignoring case, with `*` (any characters) and `?` (one character) as wildcards.
  **Mail and posts** (full-text index) need all the words in any order, ignoring case and accents; every word matches the start of a word
  (`meet` finds "meeting", #491), `"phrases"` stay exact, a wildcard only cuts the word (`mee?ing` is `mee*`) and a word that starts with one is dropped; `AND`, `OR`, `NOT`
  and `-` are plain text. Fields limit a word in mail (`from:` `to:` `cc:` `subject:` `body:`) and posts (`from:` `text:`
  `body:` `in:` `context:` `link:` `date:`) and are plain text in notes. The mail view's own search reads the text
  the same way as mail here (#490).
  ![The search window of Space f g: notes with the found text marked, mail and posts under them, and the count buttons](userguide/z_media/screenshots/picker-search.png)
  ![The search in the sidebar with the same groups](userguide/z_media/screenshots/sidebar-search.png)
- **Hits of notes are shown rendered** (#483): a found line is drawn with the editor's own inline rendering (bold, italic,
  strikethrough, highlight, code, links and wiki links by their shown text, tags in their colour, a task with a painted checkbox,
  a list item with a painted dot, a heading without its `#`), in the picker and the sidebar, and the **found text is marked** in
  raw and rendered lines (also a wildcard match, over its whole length). Mail and post hits stay plain text with the words
  of the query marked (where a word of the text starts with them). Settings → Search → "Hits of notes" (`search.hit_display`): follow the editor (default: rendered with
  the Markdown preview on or the note in view in the reading view, raw otherwise), raw, or rendered. Each hit's rendering
  is made once and kept, a hit shows at most 120 characters.
  ![Settings → Search: the sources, the look of the hits of notes and the prefix letters](userguide/z_media/screenshots/settings-search.png)

## Drawings

![A drawing with the style panel](userguide/z_media/screenshots/drawing.png)

- Hand-drawn style sketches, saved as standard `.excalidraw` files.
- Drawings of the Obsidian Excalidraw plugin (`.excalidraw.md`, compressed or not) open in the
  drawing editor too, and are saved in the plugin's own format, so Obsidian keeps reading them.
  Links like `[[Drawing.excalidraw]]` and embeds `![[Drawing.excalidraw]]` find them.
- Links inside drawings: type `[[` in a label or text for note suggestions, `[[p ` for people
  and `[!` for the file picker, just like in notes. A shape or text with a link gets a small ↗
  mark; click it (or Ctrl+click the shape) to open the note, Shift for a new tab.
- Links in drawings and canvases count like links in notes: the linked note lists the drawing
  under Backlinks, and the graph shows the connection. Renaming or moving a note updates the
  links in drawings too; canvases are never changed, so the message names any canvas that still
  points to the old name.
- Pictures in drawings show up: pictures of Obsidian Excalidraw drawings (from the vault) and of
  excalidraw.com files (stored in the file), also in drawings embedded in notes and in PDF
  export.
- Tools on keys 1–0: hand, select, rectangle, diamond, ellipse, arrow, line, pen, text, eraser.
- The style panel sets stroke and fill colours, fill style, stroke width, outline pattern,
  sloppiness, corners, label font, size, colour, background and alignment, opacity, and the
  order of shapes.
- Double-click a shape to label it. Arrows snap to shapes and follow them when shapes move.
  Hold Shift to keep lines at 45° steps.
- "Auto arrange" untangles a diagram; "Spread out" and "Pull together" change the spacing.

### Obsidian canvases

![A canvas shown read-only](userguide/z_media/screenshots/canvas.png)

- Obsidian Canvas files (`.canvas`) open in the drawing view: cards with their colours, groups,
  file and web cards (click them to follow), and the arrows between cards with their labels.
- Opening one asks whether to convert it. **Convert** makes an editable drawing
  `Name.excalidraw.md` next to it (Obsidian opens it with the Excalidraw plugin); **Open
  read-only** just shows it. The canvas file itself is never changed. Opening the canvas again
  offers the drawing made before.
- Tick "Always convert canvases when opening them" (also in Settings → Editor → Drawings) to skip the
  question.

![The question when a canvas is opened](userguide/z_media/screenshots/canvas-convert.png)

## To-do list across all notes

![Tasks](userguide/z_media/screenshots/tasks.png)

- Every `- [ ] task` from every note in one list. Ticking a task off saves it in its note.
- Filter by text or `#tag`, show done tasks, or only tasks with a due date
  (`📅 2026-12-01` or `due:2026-12-01`).

## Linked notes and the graph

![Graph](userguide/z_media/screenshots/graph.png)

- The right sidebar shows backlinks, unlinked mentions, outgoing links, the outline (headings with their formatting and numbers, as in the editor) and a local
  graph of the open note.
- **Numbered headings** (LINK-014, CFG-042): the headings of a note show a number before their text, `#` 1, `##` 1.1, `##` 1.2, `#` 2; the note's title is the title line above
  the text (below), not a heading, so a `#` is numbered like the others. The numbers are only drawn (muted), never written into the file: in the editor, the Markdown preview, the reading view, the outline, the link preview and
  the PDF. Settings → Editor → "Number headings" (on by default) switches it for every note; a note's front matter
  `numbered-headings: true` or `false` (also `yes` / `no`) overrides it for that note.
- **Properties as a summary and a card** (EDIT-060 to EDIT-064, CFG-050 to CFG-053): the reading view draws the front matter as a card by the type of the note instead of raw
  lines: an e-mail as a mail header (sender, date, to / cc, subject, tag chips), an event with its time, place, attendees and a repeats badge, a Word import as one page chip
  (`A4 · 2.5 cm margins · 11 pt`), any other note as key and value rows. Ids and `print-*` settings are left out, a ▸ toggle shows every property, and people with a contact
  note are links. `properties::summary` gives the one line for a folded view (`email · Anna Müller · 2026-11-12 · #work #mail`). Settings → Appearance → "Properties in the editor"
  / "Properties in the reading view": folded, card or raw (defaults: folded in the editor, card in the reading view). In the editor a note's front matter starts folded, to its summary line or, with the setting card, to the typed card of the reading view (a click on it opens the lines), unless the setting is raw (EDIT-081, EDIT-084). The date of an e-mail card and the start of an event card are links: a click shows that day in the calendar (EDIT-108).
- **Title line and date prefix** (VAULT-025, CFG-043): above line 1 a note shows its title line, its file name without `.md` and without a
  leading date prefix `YYMMDD_` (`261117_Plan.md` shows `Plan`); editing it renames the file and keeps the prefix. Tabs and the PDF title show the name without the prefix; the file
  list, the quick switcher and links keep the full name, so the list sorts by date. Settings → Editor → "Date prefix for new notes" (on by default) names a note made by Shale
  (new note, red link, quick switcher, new drawing) `YYMMDD_<name>.md` with today's date; a red link `[[Plan]]` that makes `261117_Plan.md` is rewritten in the note to
  `[[261117_Plan|Plan]]` so it still reaches it. Imports and daily notes keep their names.
- **Portable names** (VAULT-026, VAULT-027, VAULT-028): every file and folder name Shale makes works on Linux, macOS and Windows. A name you type for a new note, folder or drawing, or
  a rename (also from the title line), never fails for its characters: `< > : " / \ | ? *` and control characters become `-`, a trailing dot is dropped, a Windows device name like `CON`
  gets `_`, and the name is cut to 200 bytes; the toast says so ("Saved as `a-b.md`: `:` is not allowed on Windows"). A red link `[[Plan: v2]]` is rewritten to the name its note got.
  Names Shale makes itself (imports, the people notes, the media folder) stay within 127 bytes. Existing files are never renamed by this. **Settings → Vault → Check names for Windows
  and macOS…** lists, from a background job, the files and folders of the vault that would break there (forbidden characters, trailing dot or space, device names, names over 200
  bytes, paths over 240 characters, names that differ only in letter case) with the portable name for each; **Rename** does one, **Rename all…** all of them after a question,
  through the usual rename, so links are rewritten, and a name that is taken gets ` 2`, ` 3`. `.git`, `.shale` and `.trash` are skipped.
- **Lift headings on paste** (EDIT-038): pasting text whose headings are all `##` or deeper opens "The pasted text has no # heading. Lift its headings?" with **Lift** and **Paste as it
  is** (Esc pastes as it is); text without headings or with a `#` is pasted at once.
- **Heading levels bar** (EDIT-115 to EDIT-117, rabalyn/shale#457): a note whose headings skip levels (`##` first, or a `#`
  followed by a `###`) gets a thin bar above the text, in the editor and the reading view: "Headings: no level 1 heading",
  "Headings: level 2 is skipped" (up to the deepest level found; code, math blocks and front matter hold no headings). When
  the highest heading is deeper than `#`, **Move all up by X** moves every heading up by the same X in one undo step (`u` in
  Vim-style editing too); a gap that is left stays named. The **×** hides the bar for that note until Shale quits.

![The heading levels bar](userguide/z_media/screenshots/editor-heading-bar.png)
- **Heading links scroll** (LINK-015): `[[Note#Heading]]`, `[[#Heading]]` and `[text](Note.md#Heading)` open the note with the heading at the top of the view, in the mode the tab is
  in: the cursor on the heading line in the editor, the reading view scrolled to it in the reading view. The heading is found ignoring case, spaces and inline markup
  (`## **Plan** v2` matches `#Plan v2`); a missing one opens the note at its top and says so. The outline's entries do the same.
  While typing `[[Note#` or `[[#`, the suggestions list that note's headings (indented by level, with their numbers, filtered by what follows the `#`) and insert the plain text of the one you pick (LINK-016).
- The vault graph shows every note. Red dots are missing notes and green ones are drawings.

## The ribbon and the sidebars

- **App icon**: three slate plates stacked on a dark square. The window shows it in the title
  bar, taskbar or Dock; the Linux download's `install.sh` puts the desktop file and icons in place for the app menu
  and the Wayland taskbar (see the README).
- The icons of the ribbon (the narrow bar on the left) can be dragged into any order. A line
  between two icons shows where the icon lands; Esc, or letting go anywhere else, cancels. The
  sidebar toggles at the top and bottom and the settings button stay where they are.
- The right sidebar is a stack of **views**. Each view has its own menubar of panel icons
  (Backlinks, Outgoing links, Outline, Graph, Calendar, History, Thread) and shows the panel of the selected
  icon, so several panels can be open at once.
- Drag a panel icon over the content of the right sidebar: the area lights up ("Drop to open
  this icon in a new view") and dropping makes a new view below it. Drag icons from one
  menubar to another, from a view to the ribbon (it then is a shortcut that shows the panel
  below the views) and back. Icons that are no panels and no views of the left sidebar (Mail, Feeds, …) cannot go to the right
  sidebar: the pointer shows "not allowed" and nothing changes. Taking the last icon out of a
  view removes the view. Esc cancels a drag.
- **Files, Search and Tags on either side** (LAY-020 to LAY-027): drag the icon of Files, Search or Tags from the
  ribbon to a right menubar or the right sidebar's content, or from a right menubar back to the ribbon. On release
  Shale asks **Move or copy?** (Move takes it from the side it came from; Copy shows it on both sides; Cancel or Esc
  changes nothing). Right-click the icon for **Move to / Copy to the right (left) sidebar** instead of dragging. A copy
  shows the same state: one file tree, one search. If the left sidebar's view has moved, it shows the next one (or says
  none is left); Ctrl+Shift+F shows Search where it is. The places are kept in the settings (`sidebars`).
- Drag the border between two views up or down to change their heights (they start equal; a
  view never gets lower than a small minimum).
- A middle click on the **Mail**, **Feeds** or **Calendar** icon opens that view in a new tab.
- Right-click an icon (in the ribbon or in a view's menubar) for a menu: **Remove from the
  ribbon** / **Remove from this view**, **Restore …** for every icon removed earlier, and
  **Reset layout** (the default icons, order, heights and sidebar widths). The last icon of the ribbon cannot
  be removed. Panel shortcuts in the ribbon have their own glyphs, not those of the Graph and
  Calendar buttons.
- The layout (icons, heights, removed icons) and the widths of the left and the right sidebar (drag their borders) are kept in the settings.
- **F1** (or Settings → Help → **Open user guide**) opens the user guide, itself a Shale vault:
  The guide is built into the program, which unpacks it once per version into its data folder (a
  copy you edited is not overwritten) and opens it at "Start here". This works in every build on
  every system; the release also has it as `shale-userguide-<version>.zip` and, to print or read
  anywhere, as one PDF, `shale-userguide-<version>.pdf`.

![The right sidebar split into two views: Backlinks above, Calendar below](userguide/z_media/screenshots/sidebar-split.png)

## Calendar

![Month view](userguide/z_media/screenshots/calendar-month.png)

- A calendar in the right sidebar (small month) and as a big tab, with Month, Week and Day views
  in three sizes.
- Local calendars (`.ics` files, for testing; with sample events) and **CalDAV** calendars
  (for example Nextcloud). Each CalDAV calendar has its own automatic sync interval (15 minutes
  by default).

![Week view and event details](userguide/z_media/screenshots/calendar-week.png)

- Click an event to see its details: when, how it repeats, where, organizer, attendees,
  categories, reminders and notes. **Edit** and **Delete** work for local and CalDAV calendars. Delete asks first ("Delete
  “Title”?"), and a copy of the event is kept in `calendar-trash` in the data folder. A CalDAV
  save or delete that cannot be sure the server still has what Shale read is refused.
- A repeating event asks what you mean: **Edit this date** or **Edit all repeats**, **Delete this
  date** or **Delete all repeats**. A date you change becomes its own entry next to the series; a
  date you delete is left out of the series.
- Moving an event's start in the event dialog (typed or picked) moves its end along, so the
  event keeps its length; changing the end never moves the start.
- The event dialog sets how an event **repeats** (daily, weekly on the days you pick, monthly,
  yearly; every N; ending never, on a date or after N times), the dates it **skips**,
  **reminders** (minutes, hours or days before), its status, whether it shows as busy or free,
  categories and the time zone its times are stored in. Shale stores and edits reminders but
  does not ring itself; your phone or another calendar program does. Every date can be typed or
  picked from a small month.
- **Nothing is lost when you save.** Shale writes back every line of an event that it read and
  you did not change: reminders and repeat rules made in other programs, who accepted an
  invitation, time zones, and fields Shale has never heard of. A repeating event keeps its
  clock time over summer time in every program.

![New event](userguide/z_media/screenshots/event-dialog.png)

- Double-click a day or time to create an event. It starts at the next full or half hour and is
  30 minutes long.
- "Save to" chooses any calendar. When a calendar refuses the save (for example because you have
  no write permission), the error is shown in the dialog and you can pick another calendar.
  Moving an event writes it to the new calendar first and removes it from the old one only after
  that worked, so nothing is lost.

![Calendar settings](userguide/z_media/screenshots/calendar-settings.png)

- Settings → Calendar: add local calendars or CalDAV accounts ("Find calendars" asks the
  server), choose colours, show or hide calendars, and set the sync interval per calendar. **Edit**
  on a CalDAV calendar changes its address, user name or password (empty password = keep the saved
  one); a new user name or password also goes to the other calendars of the same server and user.

In a narrow sidebar the header wraps: the month name on its own line, the buttons below it, never over it (LAY-019).

![Calendar in the sidebar](userguide/z_media/screenshots/calendar-sidebar.png)

## Mail

![The mail view](userguide/z_media/screenshots/mail-view.png)

- The **Mail** button in the ribbon (or Ctrl+Shift+M) opens a three-pane view: accounts and
  folders with their unread counts, the list of messages (newest first) and the message itself
  as plain text. Shale reads your mail over IMAP and keeps a local copy next to the other
  program data (`~/.local/share/shale/mail`), so it works offline and search is instant.
- **Syncing only reads; what you do yourself is sent.** Fetching mail never changes anything on
  the server (it does not even mark mail as read). Only your own actions are written back: mark
  read or unread, flag, move to another folder, delete (to Trash) and, inside Trash, delete
  forever. Passwords are checked over a TLS connection with the system's certificate store; there
  is no unencrypted option.
- **No HTML, no tracking.** The message text comes from the plain text part, or from the HTML
  turned into text. Nothing is rendered, no pictures are fetched, no scripts run. An attachment
  is opened from a private temporary copy through your default program (never a program or
  script), or saved into the vault's media folder. A sound (every format of the media list, also a
  `.wav` sent as a plain file) gets the player of the notes under its line in the reading pane,
  playing from a private temporary copy and never by itself; sounds you attach go out with their
  `audio/*` type.
- **The local copy is not encrypted** (like Thunderbird's). Use full-disk encryption.

![Mail accounts](userguide/z_media/screenshots/mail-accounts.png)

- **Accounts…** adds an account: server, port (993 with TLS, 143 with STARTTLS), user name and
  password. The password goes into the system keyring (the Keychain on macOS, the Credential Manager on Windows). Without a keyring Shale says so and asks
  again each time it starts, keeping it in memory only; it is never written to the settings
  file. **Test connection** checks the login and lists the folders. If you change the server or
  the user name of an account, it is another mailbox: it starts with an empty local copy, you
  enter the password (or sign in) again, and the mail copied from the old one stays on disk.
- **OAuth2 sign-in** (Google, Microsoft, or your own provider): choose "OAuth2 (in the browser)",
  the provider and your own **client ID** (Shale ships none; see below), press **Sign in in the
  browser**, and sign in on the provider's page. Shale shows who signed in, keeps only a refresh
  token in the system keyring and renews the short-lived access token by itself. If the sign-in
  expires or is revoked, the mail view and the settings say so and **Sign in again** fixes it.
  Without a keyring the sign-in lasts until Shale quits. If a renewed sign-in can't be saved in the keyring, Shale says so once.
  - *Google:* in the Google Cloud console create a project, set up the OAuth consent screen
    (External, status Testing) and add your Gmail address as a test user, create an **OAuth client
    ID of type "Desktop app"**, and enter its client ID and client secret in Shale. The scope
    `https://mail.google.com/` is "restricted": an unverified app is limited to 100 test users, and
    in Testing status Google ends the sign-in after 7 days (sign in again).
  - *Microsoft:* in the Entra admin center register an app ("Accounts in any organizational
    directory and personal Microsoft accounts"), add the platform "Mobile and desktop
    applications" with the redirect address `http://localhost`, switch on "Allow public client
    flows", add the delegated permission `IMAP.AccessAsUser.All`, and enter the Application
    (client) ID in Shale (no secret). A work account may need IMAP switched on by its admin.
  - *Custom:* any OIDC provider with an authorization-code + PKCE flow; type its authorization and
    token addresses and the scope. `http://` is accepted only for `localhost`.
- The first sync fetches the mail of the last three years, newest first, several folders at once,
  and can be stopped and resumed. **Sync** and the status bar show the progress; new mail is also
  fetched every 10 minutes (changeable, 0 = off) while Shale runs. A mail deleted on the server
  stays here, dimmed, with a note; **Delete local copy…** (the button in the open mail and in the
  bar of the marked mails, the row menu, or Shift+D) removes Shale's own copy of such mails, after
  you confirm, several at once. Mail that is still on the server never gets it. The server and the
  notes you imported from the mail are not touched, and the file goes to the local `.trash`.
- **Changes in another mail program show up within seconds.** While the mail view is open, the
  folder you have open and each account's Inbox are watched over a read-only connection with IMAP
  IDLE: a flag removed, mail read, new or deleted mail in Thunderbird or on your phone appears here
  without pressing Sync (a server without IDLE is asked once a minute). Leaving the mail view
  closes the connections; a wrong password is not tried over and over.
- Choose which folders are synced with the tick boxes. **Trash, Junk and Gmail's "All Mail" are
  not synced automatically**: when you open one, its list is loaded from the server, and a
  message is downloaded when you press **Load this message**. The same button appears for a
  message over the size limit (200 MB).
- **Search** (the field above the list, or `/`): words, `"phrases"`, `from:`, `to:`, `cc:`, `subject:`
  and `body:`; ignores case and accents; every word matches the start of a word (`meet` finds
  "meeting") and `*` / `?` work as in Space f g. Choose all folders or only the open one.
- **Import to vault** (button, or `i`) hands the mail to the same Import dialog as a dragged
  `.eml` file: attachments, people and tags work the same, a mail that was imported before is
  recognised, and nothing is written until you press Import. Mark several mails (x or
  Ctrl+click) to import them together.
- **Keyboard first.** One of the three panes has the keyboard and is framed in the accent
  colour; Ctrl+H / Ctrl+L (Cmd+Shift+H / Cmd+Shift+L on macOS; the keys that go between the file list and the editor) move it between
  folders, list and message, and from the folders on into the left sidebar. The three panes keep
  a least width (170, 220 and 280 points): in a window too narrow for them the panes scroll
  sideways and the pane that has the keyboard is scrolled into view. j / k act in that pane
  only: folder, message or scroll. `/` goes to the search field (↓ or Enter there go into the
  results), Esc steps back. Everything has a key (see the Keyboard section), also syncing, the
  attachments, loading a message that is not here and switching the search scope, and all of them
  can be changed in Settings → Keybindings (Mail view). Keys do not act while you type. The row
  menu shows the key of each entry, and the tooltip of a button names its key (the buttons of an outbox mail show it on the button too); all follow what you
  set in Keybindings.
- **Read, flag, move, delete** (also with the buttons above the message, a right click on a row,
  and the buttons that appear when mails are marked). The keys work on the marked mails, else on
  the open one, else on the one under the cursor, and need no Ctrl or Shift:
  `u` marks read or unread, `s` or `f` sets or clears the flag, `d` or Delete moves to Trash (in
  Trash it asks whether to delete forever; Enter does not confirm, only the red button does), `m`
  opens the folder picker (type to find a folder, ↑ ↓, Enter; Esc leaves). Unread mail is bold,
  flagged mail has a flag, and anything the server has not confirmed yet says "saving…",
  "moving…" or "deleting…" and shows in the bar as "N changes are waiting for the server".
- **Safe by design.** A change shows at once and is sent in the background. Without a network it
  waits (also across a restart) and goes out when the server answers; if the server refuses, the
  change is undone in the list and you are told why. "Delete" is always a move to the Trash
  folder. "Delete forever" removes only the mails you chose (never the ones another program
  marked as deleted). A mail that another program moved or deleted meanwhile is left alone and
  dimmed. Shale never deletes a file of its own local copy: files of mail that moved or were deleted go to
  `.trash/<date>/` inside the mail folder. A server without `MOVE` is handled by a verified copy
  first; a server without `UIDPLUS` cannot delete forever, and for it deleting without `MOVE` is
  refused rather than risked.
- **Mark as read when opened** (Accounts…): on by default, after 2 seconds (0 = at once). Mail
  you close before then stays unread. Off: opening never changes the "read" mark.

### Writing and sending (phase 3)

- **Compose** with `c` (or the Compose button), **Reply** `r`, **Reply all** `Shift+R`, **Forward**
  `Shift+F` (also buttons under the message, which also answer to Alt+R, Alt+A, Alt+F and, for the others, Alt+I, Alt+C, Alt+U, Alt+G, Alt+M, Alt+D (not on macOS, where you can bind them); hold Alt to see the letter underlined; `f` is still the flag key). Mail is plain text. To, Cc
  and Bcc complete from the addresses in your people notes and in the mail you already have (↑ ↓,
  Tab or a click); the subject, the text and the attachments are below. Attach a file by its path,
  a file of the vault (also from the media folder) or a **note** (its markdown, as a `.md`
  attachment). An answer quotes the mail with "On <date>, <name> wrote:" and `> `, sets
  `In-Reply-To` and `References` (the whole chain), and a forward brings the attachments along.
  Your signature goes below. **Ctrl+Enter** sends, **Esc** closes.
- **Set up sending** in the account settings, "Sending (SMTP)": server, TLS (465) or STARTTLS (587),
  your name and address, signature. The sign-in is the one of the account (the same password, or the
  same OAuth2 sign-in; Google and Microsoft fill in the server, Microsoft's permission list gets
  `SMTP.Send`). There is no unencrypted option, and with STARTTLS nothing, not even the password, goes
  before the encryption. "Test connection" also signs in to the SMTP server (it never sends a mail).
  The wait before sending ("Undo send", 10 seconds, 0 = at once) is set there too.
- **Never lost, never twice.** What you write is saved as a draft after a pause in typing and when
  you close the window (`~/.local/share/shale/mail/drafts/`, as a `.eml` any program can read, plus
  the fields and attachments); after a crash the drafts are in **Drafts**. Closing with changes asks
  "Keep draft?"; a discarded draft goes to `.trash/`, nothing is deleted. **Send** puts the mail into the
  **Outbox** (a file you can read), and for a few seconds a bar offers **Undo send**, which brings it back
  into the editor. Then it is sent, once. If the server refuses (a wrong address, no network) it stays in
  the Outbox with the server's words and you press **Retry**, **Edit** or **Discard**: nothing is retried by
  itself. If the connection ends *after* the whole mail was handed over and before the server answered,
  nobody can know whether it went out: the mail shows as "may have been sent", Shale never sends it
  again by itself, and you look in Sent and choose **Send again** or **Discard**. The same if Shale was
  closed in the middle of sending.

  ![An outbox mail that was not sent](userguide/z_media/screenshots/outbox-failed.png)
  ![An outbox mail that may have been sent](userguide/z_media/screenshots/outbox-unsure.png)

- **Questions before sending**: with no subject, with no text, and when the text says "attached" /
  "attachment" / "angehängt" / "anbei" / "Anhang" but nothing is attached. Bcc recipients get the mail
  but are never written into it; no read receipt is requested; the `User-Agent` header is off unless
  you switch it on (privacy).
- **The Sent copy**: after a send, the exact message appears in your Sent folder at once and is saved
  to the server in the background (an account that keeps sent mail itself, like Gmail, is switched off
  from that). If that save fails the mail is still sent, never sent again, and the copy is retried.

### Signed and encrypted mail (phase 4)

Shale uses the **GnuPG on your computer**: `gpg` for OpenPGP (PGP/MIME) and `gpgsm` for S/MIME, with
your own keys, smartcards and the usual passphrase dialog (`gpg-agent` / pinentry). Shale never sees a
key or a passphrase, never looks a key up on the network by itself, and tells you in words if `gpg` or
`gpgsm` is missing.

- **Reading.** A strip above the mail says what GnuPG found, in its own colours, never from anything in
  the mail: a valid signature (who, which key, how far you trust it), a **bad** one (the mail was changed
  or forged), a signer whose key you do not have, an expired or revoked key, a mail that is signed only in
  part, a signature by somebody other than the sender, and inline OpenPGP (signed text in the body), which
  is marked as weaker. Checking a signature needs no setting and stores nothing. A body text that says
  "Good signature from…" is just text and shows no strip.
- **Encrypted mail** needs one decision first. In **Accounts…, Encryption** a red banner says
  **"ENCRYPTED MAILS WILL ALWAYS BE STORED DECRYPTED LOCALLY"**: a decrypted mail is kept next to the encrypted
  original (`<number>.decrypted.eml`) and its text goes into the search index, unencrypted, in the mail folder
  (protected by the folder's permissions and your full-disk encryption). It is switched on only when you type
  `I agree to decrypted local storage` exactly. Until then an encrypted mail shows "Encrypted — enable
  encryption in the settings", and nothing is decrypted or stored. Withdrawing the consent stops decrypting; it
  deletes nothing (the copies you have stay, are shown and searched). Mails are decrypted when you open them,
  in the background; a passphrase or PIN dialog may open, and Cancel stops it.
- **Safe by construction** (EFAIL): only a mail that is encrypted as a whole is decrypted; a mail that mixes an
  encrypted part with other parts or text is not decrypted and says so; decrypted text is shown as its own
  message and never joined with anything else of the mail; there is no HTML and no link is followed or
  made clickable. Signed-and-encrypted mail works in both orders and when OpenPGP does both at once.
- **Writing.** Set the key of an account in Accounts…, the account, *Signing and encrypting* (the lists come
  from `gpg --list-secret-keys` and `gpgsm --list-secret-keys`), plus "sign new mail by default" and the
  preferred standard. In a new mail: **Sign**, **Encrypt** and (with both kinds of key) the standard. For
  every address in To, Cc and Bcc the window shows whether a key was found in your local keyring (with its
  trust) or not. Encrypting needs a key for **every** recipient, Cc and Bcc included, and always also
  encrypts for you, so the copy in Sent stays readable. OpenPGP Bcc recipients are hidden recipients; S/MIME
  lists every certificate, so with a Bcc it asks first. Signing and encrypting run in the background (the
  passphrase dialog is GnuPG's), the window stays as it is and Cancel stops it; if anything fails nothing
  goes to the Outbox and nothing you typed is lost. What is signed is exactly what is sent (quoted-printable or
  base64 for anything a mail server might rewrite). The **subject and the addresses of an encrypted mail
  are not encrypted** (protected headers are not supported).
- **Your keys and Thunderbird.** Thunderbird keeps OpenPGP keys in its own store. Export your secret key in
  Thunderbird (Account settings, End-to-End Encryption, "Backup secret key to file") and run
  `gpg --import backup.asc`; the keys of others the same way, or by hand with `gpg --locate-keys name@example.org`
  (also WKD). S/MIME: Thunderbird's certificate manager, Backup, then `gpgsm --import backup.p12` and the
  issuer's fingerprint in `~/.gnupg/trustlist.txt`. The Encryption settings say all this too.
- **Keeping the key unlocked.** Without a `gpg-agent.conf`, GnuPG forgets a passphrase after 10 minutes
  without use and after 2 hours at the latest. Accounts…, Encryption, "Keeping the key unlocked" shows the
  agent's times in words (read with `gpgconf --list-options gpg-agent`, in the background) and warns when
  `no-allow-external-cache` blocks the keyring. The choice "Ask for the passphrase again after" (8 hours, 1 day,
  1 week) and "Change with gpgconf…" show exactly what would be written (`gpg-agent.conf`, the two lines, the
  marked block gpgconf makes, nothing else) and write only after "Write with gpgconf" (then the agent is
  reloaded); Cancel writes nothing. The same lines and the reload command are shown for doing it by hand, with a
  Copy button, plus the keyring tip of the platform ("Save in password manager" on Linux, "Save in Keychain" on
  macOS; Gpg4win has none). Shale still never handles a passphrase.

## Importing e-mails, events and files (for project tracking)

![Import dialog](userguide/z_media/screenshots/import-dialog.png)

- Drag e-mails from Thunderbird, events from a calendar (Shale's own or another program), `.eml`
  or `.ics` files, or any other files (plans, PDFs…) onto the window. The ribbon's Import button
  opens the same dialog.
- Pictures, PDFs, audio and video files are not made into notes: each is its own item (Image, PDF,
  Audio, Video) and is saved as a file in the media folder (`z_media/YYYY/MM/YYMMDD_name`), or, when it
  comes from outside the vault, linked where it is: each such file has a **Copy into the vault / Link
  where it is** choice (and one for all of them), starting as the setting says (MEDIA-019). Dropped
  on a note, the note gets a link line for each.

  ![The Import dialog with the choice for files from outside the vault](userguide/z_media/screenshots/import-outside.png) An image dragged out of a web browser (Wayland) is
  saved the same way, from the picture the browser hands over or downloaded from its address.
- A dropped or picked `.md` / `.markdown` file becomes a note (its text unchanged, listed as
  "Markdown note"), not an attachment (IMP-033). When it has headings but no `#`, a box "Lift headings
  (## → #, ### → ##, …)" is ticked: all headings move up so that the highest becomes `#`; code and front
  matter stay as they are (IMP-034).
- **Word files** (`.docx`) become notes through **pandoc** when you turn that on in Settings →
  Import and pandoc is installed (`sudo pacman -S pandoc-cli` / `sudo apt install pandoc`, `brew
  install pandoc`, `winget install JohnMacFarlane.Pandoc`); Shale finds it in `PATH` and the usual
  places, or you give its path (IMP-035, IMP-036). Headings, bold, italic, underline, strike-through,
  sub- and superscript, links, lists, tables (also with merged cells), quotes, footnotes, math,
  highlights and pictures with their captions come through (IMP-037). Everything one file brings
  goes into one folder, `z_media/YYYY/MM/import_docx_<name>/`: the pictures in `media/` and, when
  "Keep the original .docx" is ticked, the original, which the note names in its properties
  (IMP-038). Word's look comes along: text colours (direct, from character, paragraph and table styles, and theme colours with tint and shade) and highlight colours (yellow too) become `<span>` and `<mark>`
  colours (IMP-041), page breaks (manual, "page break before", section breaks; IMP-044), a Title and Subtitle become a heading and an italic line (IMP-042), and the
  note's properties hold the page setup (`print-paper`, `print-margins`), the body font, size,
  line and paragraph spacing, alignment, the colour, weight and sizes of the headings and, unless Heading 1 is numbered in Word, `numbered-headings: false` (`print-*`, IMP-040, IMP-045), which
  the PDF export uses (rabalyn/shale#338). Centred, right-aligned and justified paragraphs become `<div align>`
  wrappers (rabalyn/shale#340). A Word table of contents is left out (the export makes its own, IMP-043). A file pandoc cannot read leaves nothing behind and the toast says why. Off, or
  without pandoc, a `.docx` is an attachment as before; the dialog says when pandoc is missing.

  ![The Import tab of Settings](userguide/z_media/screenshots/settings-import.png)
- **Export to Word** (DOCX-001 to DOCX-010, rabalyn/shale#487): a note's ⋯ menu and its context menu in the file list have **Export to Word…**,
  the reverse of the Word import, by the same `pandoc` (the Word import setting need not be on). The dialog saves to `<vault>/exports/<name>.docx`
  and has the boxes **Use the original Word file as the template** (for a note imported with its original kept, its `source:` property),
  **Table of contents**, **Page break after the table of contents** (on; DOCX-013) and **Open the file afterwards**; the export is a background job that Cancel stops, and nothing is written when it
  fails. Shale cleans the note's text (front matter off, embeds by their vault path), pandoc reads it with commonmark and a Lua filter of
  Shale's (highlights, text and background colours, underline, sub- and superscript, strike-through, line breaks, links to notes as their words,
  pictures as wide as in the editor with their captions as alternative text, page breaks, alignment wrappers, the import's title block as Word's
  Title and Subtitle), with a reference document that is a copy of the original (pandoc's own styles merged into its styles) or pandoc's default
  with the note's `print-*` properties laid into its styles (font, size, spacing, justified, heading colour, weight and sizes, headings that
  start a page), and Shale finishes the file (alignment, colour styles, the page and margins of the note in the last section, unused pictures
  out, compressed). Docx → note → docx keeps what the import kept: the words of the fixtures come back (all of them, 95 % of `text.docx`);
  merged cells, figure and table numbers, tab stops, line spacing of single paragraphs, small caps and all but the last section's page are lost
  at import. `scripts/roundtrip-docx.sh <file.docx>` prints the share of the words of any Word file that comes back.
  `scripts/validate-docx.sh <file.docx>…` checks a Word file against Microsoft's WordprocessingML schema (the export tests do it for every file they
  write) and `scripts/docx-corpus.sh` runs the round trip, the schema check and a page count on 50 real-world Word files (developer tools). A Word
  file whose XML is nested more than 256 levels is refused at import with a message instead of crashing the program. Every pandoc run of
  the import and the export is stopped after 120 seconds and when it needs more than 2 GB of memory (`GHCRTS=-M2g`), with a message and nothing written.

- Choose the folder (one per project, for example) and tags. Small pictures from signatures are
  left out by default, and each attachment can be ticked on or off. The signature files of a signed
  mail (`smime.p7s`, `signature.asc`) and the pictures of a signature block (logos, social icons,
  `image001.png`) start unticked and say why; the box **Skip signatures** (Import dialog, start value
  in Settings → Import) lets them start ticked (IMP-059, IMP-060, IMP-061, CFG-074).
- The text of an imported mail is read as **segments**: the sender's own text and each mail it quotes
  (`>` runs, "On … wrote:", "Am … schrieb …:", `-----Original Message-----`, Outlook's From / Sent / To /
  Subject block, `blockquote` and `gmail_quote` of HTML mails), and each segment has its own signature
  (`-- ` at any quote depth, the HTML markers of Thunderbird, Gmail, Outlook and Apple Mail, a closing
  greeting with a short block of contact lines, a disclaimer). **Skip signatures** also removes the own
  signature and the signatures inside the quoted mails and keeps the quoted text; **Leave out quoted
  mails** (off at first, Settings → Import sets the start value) drops the quoted mails. What is not
  clearly a signature stays (IMP-063 to IMP-066, CFG-076). The editor folds a signature inside a quote
  too (EDIT-083).
- **A follow-up mail continues its thread** (ADR-0032, #575). A mail note has `in-reply-to`, `references`
  and `thread-id` besides `message-id` (IMP-067). The Import dialog looks in the vault for notes of the same
  thread by the reply headers only (never the subject, sender or text) and offers "Continues [[note]]" with a
  tick box and, with several matches, a choice (IMP-068); the folder of the import follows the matched
  note unless a folder was dropped on or typed (IMP-069). Ticked, the new note gets `previous` / `next`
  links in date order and the last note of the thread its `next`: one line of one other note (IMP-070). An
  older mail asks "Keep the thread in date order?" with **Update** or **Leave the chain**; an import never
  rewrites more than two other notes, open notes are saved first (IMP-071). The new note and the neighbour
  lines are one history snapshot (IMP-072); a rename keeps the chain because `previous` and `next` are
  links, and old notes with only a `message-id` still match as the parent (IMP-073).
- **Thread view** (NAV-070, KEY-050): a panel of the right sidebar lists the notes with the same
  `thread-id` as the open note, oldest first, with date, sender and title and the current one marked; a
  click opens the note (a pinned tab is not replaced). Ctrl+Shift+PageUp / Ctrl+Shift+PageDown step to
  the previous / next mail of the thread, Ctrl+Shift+T opens the view and gives it the keyboard.
- Drop onto a folder in the file list to import into that folder, or onto a note to also add
  links to the new notes there.
- After an import of two or more items a window, **Import summary**, says what was made: the new
  notes, each with its name, a line about it (an e-mail: the sender and the date; an event: the start
  and the place; any other note: its first line) and an **Open** button, the files saved in the media
  folder, and the errors. **Open all in tabs** opens every new note in a tab of its own; Close leaves
  them closed. One item is opened right away as before (IMP-056, IMP-057).

  ![The summary after an import](userguide/z_media/screenshots/import-summary.png)
- Messages that were already imported are recognised and left out unless you tick them again.
- On Wayland, Shale reads what Thunderbird hands over. When that is only a link to the message,
  Shale finds the message in Thunderbird's local copy (for IMAP accounts this needs "Keep
  messages for this account on this computer").

- A calendar event becomes a note without dragging: the **Import** button of its details popup (left click) and the **Import** entry of its right-click menu open the same Import dialog as a drop, for a repeating event with the clicked date (CAL-034).

![Dragging an event onto a folder](userguide/z_media/screenshots/drag-event.png)

![An imported e-mail](userguide/z_media/screenshots/imported-note.png)

- Each e-mail becomes a note named "date subject". The sender, recipients, date, the people (as
  links to their contact notes), a link to the original message and the tags are its properties; the
  text starts with the subject as a heading, then the attachments and the message. The header is
  written once, as properties (IMP-050, IMP-051). An event is the same: start, end, place, calendar,
  repeat rule, organizer, attendees, status and link are properties, the text is its title and notes.
  The tag list counts the tags too. For programs that do not show properties, the box **Header list
  in the note body** (Import dialog; its start value is in Settings → Import) also writes the old list
  of From / To / Date / When / Where … below the title (IMP-052); a saved post gets Author / Posted /
  Source … (IMP-058).
- The text of an imported e-mail is tidied: runs of three or more blank lines become one, trailing
  spaces go, and a long tracking URL (more than 80 characters, with a query string) becomes a short
  link `[host](url)` (IMP-053). A picture among the attachments is an embed 200 points wide
  (`![[file.jpg|200]]`), other attachments stay links (IMP-054). The signature starts at a line that
  is exactly `-- ` and quoted replies are runs of `>` lines (IMP-055).

## Feeds: read Mastodon and Bluesky, save posts to the vault

![The feed view](userguide/z_media/screenshots/social-feed.png)

- The ribbon's speech-bubble button opens **Feeds** in a **new tab** every time (pick the account
  and feed there, so a new feed can join the tabs you have). **Accounts…** adds a Mastodon account (type
  the server, open the sign-in page in the browser, paste the code it shows; or paste an access
  token) or a Bluesky account (handle and an **app password** from Settings → Privacy and
  security → App passwords, never your main password). Secrets go to the system keyring. **Edit** next to an account signs in
  to it again (new app password or code) and replaces its secret; the account keeps its feeds and posts.
- Pick the account and the feed (Mastodon: Home, Notifications, Your posts and your lists; Bluesky:
  Following, Notifications, Your posts and your saved feeds). Posts show the avatar, name, time, "boosted by", pictures (video and GIF
  previews have a play mark), quoted posts and link cards. A content warning stays closed until
  you click Show. The next page loads when you reach the end of the list.
- **Several feeds at once**: every Feeds tab has its own feed, scroll position and back/forward
  history. Next to each feed in the picker, **New tab** (or Shift+click / middle-click on the
  entry) opens it in another tab; the same feed may be open twice. The tab is called
  "Mastodon · Home" and gets "(12)" when 12 new posts wait. Open Feeds tabs come back at the next
  start.
- **New posts wait, they do not jump in**: every 5 minutes (change it in **Accounts…**, 0 =
  never) each feed that is open in a tab, also in the background, looks for newer posts. They
  appear as a button "12 new posts — show" in a row above the list (or press `.`); the row is
  always there ("Up to date · checked 2 min ago" when nothing waits), so the list never moves
  and nothing is covered, and it does not change until you click. Then the new posts go above and the post you were reading stays
  exactly where it is, so you can scroll up slowly. **Check now** looks right away (a spinner shows inside the button while it runs);
  **⋯ → Reload from the top** replaces the list. If more than 200 posts came in on Bluesky,
  **Load more in between** fills the gap.
- **Notifications are a feed** (Mastodon and Bluesky, in the picker of every account): one compact row
  per notification (what happened, who did it, how long ago, a dot while it is unread; "Ada, Bo and 3
  others favourited your post" when likes or boosts of one post come together, also when a page or a check cut the group) above the post it
  is about as a normal card with all its buttons (reply, like, boost, quote, Show thread, Save to
  vault …). A follow shows the account and **Open in browser**. Mentions, replies, quotes, favourites
  / likes, boosts / reposts, follows and follow requests, polls, edits and posts you asked to be told about are
  shown. The feed is checked like the others; new ones wait behind "N new notifications — show", the
  tab title counts the unread ones, and when you have the list on screen Shale tells the server that
  you read them (Mastodon's marker, Bluesky's "seen"), so other apps agree. Their posts are in the search index.
  **Your posts** is the feed of your own posts, replies and boosts, with the same buttons; a post you
  write from Shale waits there too.

![The Notifications feed](userguide/z_media/screenshots/social-notifications.png)

- **Search the posts you loaded**: every post a feed brings in is kept in a search index
  (`~/.local/share/shale/social-posts.db`, the same for every vault: text, author, date, links
  and the thread around it). The sidebar search (Ctrl+Shift+F) lists the posts that match under
  the notes and mail (as `Space f g` does); click one to open a new Feeds tab scrolled to it, also when the feed no longer has
  it. `from:ada`, `text:`, `in:` (quote, link card, the post answered, who boosted), `link:` and
  `date:2026-11` limit a word to one part. At most 10 000 posts per account and one year are
  kept; removing an account removes its posts. Posts you save to the vault (the thread and the
  replies below it) are searchable too, a reply knows the post it answers even when that arrives
  later, and a hit whose post is gone from both index and feed is fetched from the server.
  **Settings → Feeds → Post search index** shows how many posts are indexed, **Rebuild**s it
  from the posts your open feeds hold, or **Clear…**s it (after a question). (Mouse only for now.)
- **The conversation of a post** opens in the same tab with a click on its reply count or **Show
  thread** (Shift+click or middle click: a new tab): what it replies to above, the post highlighted,
  and the replies below as a tree, moved in for each level (up to 6). It is fetched in the
  background (spinner, an error with **Retry**) and drawn with the cards of the feed: pictures and
  the viewer, links, content warnings, Save to vault, Open in browser and Copy link. **← Back** (or
  the tab's Back) returns to the feed at the same place; another post's reply count opens that
  conversation as the next step. Replies the server leaves out are noted, with Open in browser.

![The conversation of a post](userguide/z_media/screenshots/social-thread.png)

- **Pictures and videos open big in a viewer** over the tab (click one): fitted to the window, its description
  (alt text) below, **◀ ▶** or Left / Right to step through the post's pictures and videos, mouse wheel or
  `+` `-` to zoom, **100%** / **Fit** (`0`), drag to pan, Esc or **Close** to leave. The full size file
  comes through the background picture cache (spinner while it loads, a message when it fails);
  pictures behind a content warning stay closed until Show. **A video or GIF plays in the viewer** (SOC-076): the file
  is fetched into the same cache (https only, no login, at most 64 MB; a Bluesky video as its real file from the
  author's server), then plays in the media player's card with play / pause (also **Space**), seek and volume;
  a GIF plays muted and again and again. Stepping away or closing stops it. Without ffmpeg the viewer shows the
  video's preview picture and says so, and **Open in browser** stays.
- **Save to vault** copies the post, and the whole thread when it is a thread by one author,
  into the Import dialog: choose the folder (default `Social`), tags and which pictures to
  bring along. **Open in browser** and **Copy link** are next to it.

![Saving two posts](userguide/z_media/screenshots/social-import.png)

- Tick **Include replies from others (N)** to add a "Replies" section with other people's
  answers, nested like a conversation (off by default; Shale remembers your choice). Their
  pictures are linked, not downloaded.
- The note has the author, the date and the link back to the post (**Source**) as properties,
  and the text (a thread: every post in order, numbered); the box **Header list in the note
  body** also writes them as a list below the title (IMP-058). The pictures are saved in `z_media/`, with their descriptions. A post
  that was saved before, also as part of a thread, is marked "Imported before".
- **Like, boost, reply, quote, post**: every post has **Reply**, **Boost** / **Repost**,
  **Favourite** / **Like** and **Quote** buttons (also in the conversation view), and the toolbar
  has **New post**. Boost and Favourite / Like toggle (a second press takes it back), show whether
  you did it (a filled button), and change the counts only when the server said yes; a spinner
  inside the button shows while it runs and nothing moves. A quoted post inside a card has the same
  buttons in a small row (Reply, Boost, Favourite, Quote, Show thread, Save, Open, Copy link) that act on the quoted post
  itself (#588). A like or boost is also written into the post search index, so a post opened from the search shows
  the state you left it in. The compose box has the text with a
  live counter (Mastodon: the server's limit, 500 by default, addresses count 23; Bluesky: 300
  characters), on Mastodon a content warning and who may see the post (a reply starts with the
  author's @mention and is no more public than the post it answers), up to **4 pictures with a
  description each** (**Choose pictures…** opens the system's file dialog, or drop picture files
  on the box, or paste a picture with Ctrl+V, or type the path of a file or of one in the vault;
  JPEG, PNG, GIF, WebP; Bluesky takes 1 MB, so bigger pictures are scaled down) or **one video** (MP4, QuickTime, WebM, MPEG; up to
  what the server allows and 100 MB; on Bluesky it goes through the video service, with its size sent along and a video over 10 minutes refused when ffmpeg is installed), a **language** for the post (starts with the
  setting Post language), on Mastodon a **poll** (options, duration, several choices, hidden totals; not together with
  pictures or a video) and **Schedule…** to let the server send the post later (a date picked in a month grid or typed, an hour and a minute, local time, at least 5 minutes ahead;
  **Scheduled posts…** lists them and cancels one), and **Send** (Ctrl+Enter), which
  always asks **"Post this?"** with your handle, who may see it and the time of a scheduled post before anything is posted.
  Mastodon **polls on cards** show their options with bars and percentages (after you voted, when the poll is closed and on your
  own posts) or a Vote button with a spinner (#589).
  Boosts and favourites do not ask. A **quote** uses the network's own quote (Bluesky; Mastodon
  4.5 and later); for an older Mastodon server the post's web address is put into the text and
  the box says so. Bluesky links, mentions and hashtags become clickable. When it is sent the box
  closes and the post waits behind the "new posts" button (the view does not move); a failed send
  keeps your text. **Mastodon accounts need the permission to write**: new sign-ins ask for it;
  an account you signed in to before with read-only access keeps reading, and the first like,
  boost, reply or post says "Sign in again to allow writing" and opens that account's **Edit**
  sign-in.

![The compose box](userguide/z_media/screenshots/social-compose.png)

![A poll on a card](userguide/z_media/screenshots/social-poll.png)

- Try it without an account: `shale --screenshot out.png --view social` shows made-up posts.

## People (contact notes)

![People list](userguide/z_media/screenshots/people-picker.png)

- Contact notes live in the **people folder** (default `Adressbuch`, changeable in Settings).
- Type `[[p ` and a name to pick a person from that folder. The link that is written is a normal
  `[[Name]]` link, so other markdown tools understand it too. "New contact" makes a contact note
  from a template (e-mail, phone, organization, notes).
- When importing, the sender, recipients, organizer and attendees are found and matched to
  contact notes (by e-mail address anywhere in the note, else by name; "Müller, Hans" = "Hans
  Müller"). Known contacts and new senders are ticked; for ticked people without a note one is
  made. The imported note gets a `people` property with links, so each contact note's backlinks show
  every mail and meeting with that person. Automatic senders like "noreply" are left out.

![Import with people and tags](userguide/z_media/screenshots/people-import.png)

## Tags found automatically (no server, no AI)

- Tags of the vault that are written in the imported text are set right away
  (`#lecture-hall-b` for "Lecture Hall B", the last part of nested tags too).
- Shale also learns, from the notes in the vault, which words are typical for each tag (words in
  most notes with the tag and rarely elsewhere) and suggests those tags when an import contains
  several of these words. It is plain counting: the same notes always give the same suggestions.
  It is learned again in the background now and then and kept in `.shale/tag-words.json` in the
  vault.

## Settings that travel with the vault

- **Settings** (Ctrl+, or the ribbon button) has a tab per domain in a list on the left: Vault, Editor,
  Appearance, Fonts, Sync, Calendar, Mail, Feeds, Data, Help and Keybindings. It opens on the short Vault
  page, and later on the tab you used last.
- **A search over all settings** sits at the top of Settings (Ctrl+F, or `/`, focuses it). It finds a
  setting by its name, its description (the one in the settings file) or its tab, and a key action
  by its name or its keys (`ctrl+h`, `F6`). ↓ / ↑ and Enter, or a click, go to the result: the tab
  is shown, the page scrolls to the setting and a box pulses around it for a moment (it stands
  still where animations are off). Every setting of the dialog is in one table (`settings/registry.rs`)
  that tests keep complete.

  ![Settings with the search for "cursor" and its results, each with its tab](userguide/z_media/screenshots/settings-find.png)
- The settings of the program itself are one text file, `~/.config/shale/config.toml`, with a
  description above every setting. "Open settings file" (top right of Settings) opens it in a tab (no note, no
  vault file); saving applies the values at once, a change from another editor is picked up within a
  second, and text with an error is not saved (quitting puts it in the recovery folder instead of losing it). An old `config.json` is moved over at the first start.
- Each vault keeps its own settings in `.shale/settings.json` (people folder, git sync) and its
  session in `.shale/state.json` (open notes, last import folder, recent tags), so a vault
  behaves the same on every computer. The undo histories of the notes (`u` in vim-style editing)
  are kept for 30 days (a setting) in `.shale/undo/`, one file per note; that folder stays on the computer
  and is not committed by git sync. In a vault in a cloud-synced or network folder, `state.json` and the undo
  histories are kept outside the vault, in Shale's data folder, so two computers do not overwrite each other's
  tabs, and leftover `.shale-tmp-*` files and `state.sync-conflict-*` copies are removed. Settings of the program itself (appearance, calendars,
  known vaults) stay in `~/.config/shale/config.toml`.
- The files carry a format version and are upgraded on load. A settings file that can't be read
  is kept as `….broken-<date>` and you are told; a file from a newer Shale is used but not
  changed.
- On the first start after the upgrade, the people folder, open notes, import folder and recent
  tags move from the old global settings into the vault you open.

## Data usage and caches

![The Data usage dialog](userguide/z_media/screenshots/data-usage.png)

- Settings → Data and caches → **Data usage…** (or Ctrl+Shift+D) shows what Shale keeps on disk.
  **Saved**: the vault by kind (notes, drawings, pictures, PDFs, other attachments, `.shale/`,
  `.trash/`, `.git/`), its 10 largest files and folders (a click opens the file or shows the
  folder), and your data outside the vault: mail per account, drafts, outbox, mail trash, local
  calendars, calendar trash, recovered notes and the file history of a vault that has no `.git` (with `.git` it is
  counted in the vault's `.git/`), and, when notes link media from outside the vault, **Outside media linked by this
  vault**: the files that are there, each once, with their size, and how many are not found or not set up (USE-026).
  Nothing in this part can be deleted here.
- **Caches** are what Shale can build or fetch again: social pictures, the social post index,
  the mail index, its daily backups, damaged copies of the mail index, the learned tag words, the
  undo histories of the notes of the open vault and the keyboard debug log. Each row shows its size, entries, oldest entry and last cleanup, and
  has a **Clear** button. The mail index is **rebuilt** from the message files instead (it asks
  first; not while mail syncs or a change waits for the server), and damaged copies are only
  deleted after you confirm; Clear on the undo histories asks first too (every `u` history of the
  vault goes, damaged copies included) and empties the histories of the open notes.
- How long each cache is kept is a setting (on its row, and in Settings under "How long caches
  are kept"): social pictures 30 days and 500 MB, social posts 365 days and 10 000 per account,
  the newest 3 mail index backups, damaged copies forever, undo histories 30 days, the debug log 10 MB. Every value can
  also be "forever" / "no limit".
- A cleanup runs in the background a minute after the start (when the last one is more than a
  day ago) and then daily, and 2 seconds after you change a setting. It never touches your notes,
  files or mail.

## Updates

- **Settings → Updates** (#421, ADR-0023): Shale looks on GitHub for a newer release a short while after it starts and
  every 24 hours (**Check for updates now** does not wait), and, as you choose, only tells you (**Notify**), downloads
  and checks it (**Download**) or installs it (**Install**). **Off**, and `SHALE_UPDATES=off` in the environment, mean no
  request at all and no notice. Until you choose, the first update found asks. The notice has What's new (the release
  notes), Skip this version (that version only) and Later; a downloaded update offers Install now or At next start, an
  installed one **Restart now**, which saves everything like quitting and starts the new version (UPD-001 to UPD-010,
  UPD-019, UPD-020).
- The download is checked against the release's `.sha256` file and nothing is installed when it does not match. That
  shows the file arrived whole, not who made it (authenticity is #422, UPD-022).
- What an update does depends on how this copy was installed (UPD-011 to UPD-018): the Linux folder or an `install.sh`
  install, the macOS app and the Windows folder update themselves and keep the previous program as `shale.old` until the
  new one has run (on Windows `shale.exe.old` goes at the next start); a package-managed install (AUR `shale-bin`,
  `/usr/bin`) and a development build are never touched and only get a notice; a folder that cannot be written gets
  the command to run. The macOS and Windows installs are tested through their plans on Linux only (see the spec).
  Settings shows the version, how this copy was installed and why it cannot update itself where that is so.

  ![The Updates tab of Settings](userguide/z_media/screenshots/settings-updates.png)

## Git sync

- Needs `git`; without it Shale can install one (see the next point). **Turn on sync** (Settings → Sync) is a short wizard that
  asks how you want to keep your notes safe: **History on this device only** (the file history, outside the vault, no server and
  no account), **My folder is already synced by another app** (Nextcloud, Dropbox, OneDrive, iCloud or a network drive: the files
  travel through that app, the history stays on each computer and no `.git` folder is put in the folder) or **Sync with a server**.
  The first step shows the install of git when there is none, and warns when it finds that the vault is in a network folder or in
  a folder another app syncs (a guess from the file system and from marker files such as `.dropbox` and `.stfolder`; it never
  decides anything).

  ![Turn on sync: the three ways to keep notes safe](userguide/z_media/screenshots/wizard-choose.png)

  ![The same page for a vault in a Dropbox folder: a warning that names the folder](userguide/z_media/screenshots/wizard-warning.png)

- **Sync with a server.** Pick GitHub, GitLab, Codeberg or another server (or paste a repository address), then sign in with a
  **personal access token** (the wizard links to the provider's page where you make one) or an **SSH key** (Shale uses one of
  `~/.ssh` or makes an ed25519 key and shows the public key with a **Copy** button and a link to the provider's "add a key"
  page). The token is kept in the system keyring (the Secret Service, the Keychain or the Credential Manager), never in a file, in
  `.git/config`, in the address or on a command line; git gets it from Shale's own helper (`GIT_ASKPASS`, the Shale program started
  without a window). Where the provider has an API (Forgejo, Gitea, GitHub) the wizard lists your repositories and creates a private
  one; elsewhere it links to the provider's website and takes the address. Before the first sync it says what will happen
  (an empty repository gets your notes, a repository with notes comes down, both with notes are merged and nothing is deleted),
  and the **test sync** is one real sync with the result in plain words (it worked, wrong user name or token, no network, no
  repository at that address, SSH key not accepted, certificate not trusted).

  ![Choosing the server](userguide/z_media/screenshots/wizard-server.png)

  ![Signing in with a token or an SSH key](userguide/z_media/screenshots/wizard-sign-in.png)

- **The address field.** Settings → Sync keeps the old way under **Advanced**: enter the URL of a git repository
  (`https://…`, `ssh://…`, `git@host:notes.git` or a folder path, which may have spaces in its name, like `…/Dropbox/My notes.git` or `C:\Users\Ada\OneDrive - Company\notes.git`) and press Connect (an address with a password in it, and plain `http://` or `git://` except on this computer, are refused: use the wizard's token or an SSH key), or run
  `git init` in the vault folder yourself. Shale then syncs on its own: when the vault opens,
  every few minutes (default 5, changeable) and on "Sync now".
- **A repository inside a synced folder.** A vault with a `.git` folder inside a folder another app syncs or on a network share
  gets a warning in Settings → Sync; when it has no server, **Move the repository out** copies it to the history place on this
  computer, checks the copy and only then removes `.git` from the vault. With a server nothing is offered.
- **Install git with one button.** Settings → Sync shows whether a git and a git-lfs are found (through `PATH`), whether Shale's
  own git is installed, and which git is in use. Where a git is missing, **Download and install git** offers it with the
  download size, the size on disk and the folder `<data folder>/shale/git`; nothing is downloaded before you press the
  button. **Windows:** MinGit (the official portable Git for Windows, no administrator rights) and git-lfs, each file checked
  against the SHA-256 pinned in Shale. **macOS:** git comes with Apple's command line tools, and the button opens Apple's
  installer; git-lfs is downloaded. **Linux:** the page shows the install command of your distribution (apt, dnf, pacman, zypper
  or apk) with a copy button; git-lfs is downloaded when git is there and git-lfs is not. The install is built in `git.partial`
  and renamed when it works; Cancel or a failure leaves nothing. **Remove…** deletes that folder only. A choice
  (**Automatic**: the git of this computer first, else Shale's; **The git of this computer**; **The git that Shale installs**)
  says which one runs. Updates are opt-in: the setting `git.updates` (Off by default) lets Shale look for a newer release 30
  seconds after the start and weekly, and tell you or install it; `SHALE_UPDATES=off` switches that off. See ADR-0027.

  ![Settings → Sync with git found, git-lfs missing and the offer to install it](userguide/z_media/screenshots/settings-sync.png)

- A sync commits every change (the `.shale/` files too, so settings and open notes follow you;
  only the undo histories in `.shale/undo/` stay on the computer),
  fetches, merges and pushes. Commit messages read `Sync from <computer>: 3 file(s)`, with an
  optional prefix and suffix from the settings. Without a remote, Shale only keeps a local
  history.
- Nothing is ever lost: when a note was changed on two computers, the remote version stays and
  yours is kept next to it as `Note (conflict 2026-11-14 1530).md`; a note deleted on one side
  and changed on the other is kept. Shale never force-pushes, never writes conflict markers into
  notes, and pauses if git is in the middle of something you started (a merge, a rebase).
- Edits you type while a sync runs are never overwritten: if the sync brought a new version of
  that note, your text is saved as a conflict copy next to it.
- Conflict copies are recognised (NET-015): Shale's own `Note (conflict …).md` and the copies of Syncthing, Dropbox,
  Nextcloud, ownCloud and Seafile. A copy has a "conflict" badge in the file tree and a banner above the note with
  Compare (changes, or the Markdown side by side), "Keep this one" (its text goes into the original; the old text is kept in the trash) and "Keep the
  other"; the copy goes to the trash. The original shows the same banner and a badge. While the original exists, the copy is left out of search, links, the graph,
  tasks and tags. OneDrive, iCloud and Google Drive copies (`Note 2.md`) cannot be told from a note by name.
- Passwords are git's business (credential helper or SSH key); Shale never asks for one.
- Every commit ends with the lines `Shale-Reason:` (`sync`, `merge`, or why a version was kept, see File history) and
  `Shale-Device:` (the name of the computer), after the optional suffix. The sync pushes the version commits of File
  history as they are: no squash, no rewrite.
- **A folder with its own `.git`** (SYNC-076). When one is opened or added as a vault, Shale asks whether to use git sync
  with it and syncs nothing before the answer. **Use git sync** keeps `sync.enabled` on, **Don't sync** turns it off in
  the vault's `.shale/settings.json`; Esc asks again the next time. A `.git` Shale made itself, the user guide and the
  demo vault do not ask. The answer is remembered in a file inside `.git`, which is never committed.
- **Media in Git LFS.** Audio and video files are large, so they do not belong in the git history.
  With `git-lfs` installed, a repository Shale creates (Connect on a folder that is no repository
  yet) stores media with Git LFS from the start, and for an existing repository Settings → Sync
  has the button **Store media with Git LFS**. The settings show whether git-lfs is installed
  (and where to get it if not) and whether the repository uses it. Shale sets LFS up in this
  repository only (never your global git configuration), routes mp3, ogg, oga, opus, flac, m4a,
  aac, wav, mp4, m4v, mov, webm, mkv, avi and ogv (any case) to it with a block in `.gitattributes` (your
  own lines there stay), and converts media already in the repository in one commit; the old
  versions stay in the history (`git lfs migrate` removes them, by hand). Every sync uploads the
  files to the remote's LFS store before it pushes. Other computers get small pointer files and
  download a file when it is needed: a sound or picture that is still a pointer shows "Not
  downloaded (12 MB) — Download" (pressing play downloads first, then plays; a spinner shows while
  it runs and a failure is said in words). Settings → Sync has "Download media files after every
  sync" (vault setting `sync.lfs_download`: `on_demand`, the default, or `always`). A conflict copy of a media file is stored in LFS too. On a
  computer without git-lfs, a vault that stores media with LFS is not synced (nothing is
  committed) and the settings say why.

## File history

- Shale keeps earlier versions of notes and drawings, with or without a git server, and a note can go back to
  one. A version is a git commit ("snapshot"): made when a note or drawing is saved (at most one per note
  every 10 minutes while typing, then `history.coalesce_minutes`), when it has had no change for a minute
  (`history.idle_seconds`), when its last tab closes, and when the window closes (waiting at most 3 seconds). Only saved text is committed, never an
  open buffer; a save that changes nothing makes no commit. Snapshots run in the background, one at a
  time and never while the sync runs; a busy repository is asked again later and typing is never blocked.
- **Where.** A vault with `.git` keeps them in its repository (also with sync off) and the sync pushes
  them unchanged. A vault without `.git` keeps a repository **outside the vault**, with git's separate
  git directory `<data folder>/shale/history/<vault id>` and the vault as work tree; no `.git` file or
  folder is ever written into the vault. The vault id is the file `.shale/history-id` (128 random
  bits, synced with the vault); a moved or renamed vault keeps its history, a copy of a vault gets a
  new id. A new local history starts with one snapshot of every note and drawing.
- **Connecting a server moves a local history into `.git`** (SYNC-072…SYNC-075, #485). The wizard's server
  path and Advanced → Connect, for a vault without `.git` whose history is in the data folder, copy it to
  `<vault>/.git.moving` in a background job under the tree lock (no snapshot, thinning, read or sync beside
  it), check the refs, `HEAD` and `git fsck`, rename it to `.git` (`core.bare=false`, no `core.worktree`,
  `git status` must work) and only then remove the old folder; a failure leaves the vault without `.git`
  and the history where it was. The first sync pushes the snapshots as they are to an empty server, or merges
  them (unrelated histories allowed) with a server that has notes. The wizard says in words that the versions
  moved. The history is then the vault's own and is never thinned (SYNC-069).
- **What.** Notes and drawings (`.md`, `.excalidraw`, `.excalidraw.md`, `.canvas`), not media. Never
  Shale's temporary files, the temporary and housekeeping names of cloud clients and network shares
  (`.syncthing.*.tmp`, `.stfolder`, `.stversions`, `.stignore`, `.~lock.*#`, `~$*`, `*.tmp`, `*.part`,
  `.nfs*`, `.fuse_hidden*`, `*.icloud`, `desktop.ini`, `.DS_Store`, `Thumbs.db`), or the device-local files
  of `.shale/` (`state.json`, `undo/`). In a vault repository a snapshot commits only its own paths and
  leaves other staged changes alone.
- **Messages.** A short subject and the lines `Shale-Reason:` (`save`, `idle`, `close`, `initial`,
  `sync`, `restore`, `merge`, `rename`, `delete`), `Shale-Device:`, one `Shale-Path:` per path and
  `Shale-Restored:` for a restore; a commit without them (made by another tool) is read as a subject only.
- **The history panel.** The history of the note of the focused pane is a slim column (236 points) in the empty
  right margin of the note, between the text and the right sidebar, when exactly one note is open, **Readable line length**
  is on and the note area is at least 1040 points wide; the text keeps its width. With several panes side by side, a drawing or a
  narrow window the same list is the **History** panel of the right sidebar (a clock icon, last in the default order;
  it is a panel like the outline: it can be moved, hidden and shown). The **×** of the column hides it
  (`history.show_panel`, remembered), **Show it here** in the sidebar panel or the ⋯ menu entry **History of this note**
  shows it again. The list is read in the background and again after a new version or a sync.
  ![The history in the margin of a note](userguide/z_media/screenshots/history-margin.png)
  ![The history as a panel of the right sidebar](userguide/z_media/screenshots/history-sidebar.png)
- **The list.** Grouped by day (Today, Yesterday, `12 Nov`), each version with "5 min ago" or its time, where it came from
  in words (edited here, edited on a laptop, synced from a laptop, restored version of …, merged after a conflict,
  renamed from …, deleted, first version kept, changed outside Shale), and a bar of five +/− ticks with the counts as text
  (`+12 −3`); the hover says "12 lines added, 3 removed". A binary version says "changed". The right-click menu has
  **Restore this version** and **Copy text**. With history off the panel says why and has **Open Settings → Sync**.
- **The comparison.** A click on a version opens **Changes since 11:05**, the note as it is now against that version:
  paragraphs, headings (by size), list items, table rows and quotes with inserted words highlighted and underlined and deleted
  words struck through (theme colours, never colour alone), a gutter mark **+** added, **−** removed, **~** changed on every
  changed block, and unchanged runs folded into "… 34 unchanged paragraphs" (a click opens them). The slider moves the older end
  over all versions, the **to** list picks the newer end, **Next change** and **Previous change** jump (the current one has a frame).
  **Restore this version** (undoable, the text you had is kept first), **Copy text** and **Read-only** are the actions of
  SYNC-038 and SYNC-039. Drawings show before and after thumbnails, a binary version "changed" with two placeholders.
  ![Changes since 06:00 as track changes](userguide/z_media/screenshots/history-changes.png)
- **Source side by side.** The switch **Source, side by side** shows the Markdown, old left and new right, with line numbers,
  tinted changed lines, marks between the sides and the changed words underlined or struck through; **Copy as patch** copies
  unified diff text. Only here are versions named by a short hash (a tooltip has the full one).
  ![The source side by side](userguide/z_media/screenshots/history-source.png)
- **Go back.** **Restore** snapshots the text you have, then puts the old text in the note as one new change (one
  undo step in an open note; an atomic save otherwise) and snapshots that with the version it came from;
  history is never reset or rewritten. Settings → Sync and the panel have **Restore a deleted note…**: notes deleted or moved
  to the trash, newest first, written back at their path or at `Name (restored).md`. A rename or move and moving to the trash
  make their own snapshots, so the history stays connected.
- **Settings** (Settings → Sync, **File history**): on by default for every vault (`history.enabled`),
  `history.coalesce_minutes` (10), `history.idle_seconds` (60) and `history.show_panel` (the margin column). Without a usable git, history is off and
  the page says why. Data usage lists the size of a local history.
- **Thinning** (#479, SYNC-067…SYNC-071): a history on this computer is thinned once a day (60 s after a vault opens, then every 24 h, in a background job beside no snapshot): all versions of the last 24 h, the newest of each hour up to 30 days, the newest of each day before that (UTC); restore, delete and rename snapshots, the commit before a deletion, the newest and the first stay. The rewrite builds a new history with `git commit-tree`, moves `main` with a compare-and-swap `update-ref`, then expires the reflog and runs `git gc --prune=now`. A vault's own `.git` is never rewritten. Settings → Sync says which rule applies; there is no setting. See ADR-0028.

## Busy states: spinner, what is happening, Cancel

Whatever runs longer than a moment says what it does, cannot be started twice and shows in one place (JOB-003 to JOB-015, #397, #385).

- **A spinner and what happens.** A running action shows a spinner and a short phrase that names it
  ("Syncing with git…", "Checking file names and links…", "Exporting “Handbook” to PDF…"). Where the work can stop, **Cancel**
  stands next to it; a sync is not stopped half way, so it has none.
- **No double trigger.** The buttons that start the action (Sync now, Store media with Git LFS, Check names, Export) are off
  while it runs, and say why when you point at them; a key or the timer cannot start it a second time either.
- **The indicator.** While anything runs in the background, a small indicator sits in the top right of the editor pane (in the
  row of the tabs; with panes side by side, of the rightmost one): a spinner and the text of the task, or "2 tasks running".
  Point at it, or click it to keep it open (a second click closes it): it lists every task with how long it has run and **Cancel**
  where the work can stop. It disappears when nothing runs.
  ![The indicator with two tasks listed](userguide/z_media/screenshots/busy-indicator.png)
- **Known vaults** (VAULT-029, VAULT-031, VAULT-040, CFG-075, #582): the global `config.toml` keeps a list of the vaults Shale has opened, each with a name (by default the folder's name, local to the computer) and its canonical path, so a folder is never listed twice and the list has no length cap. It replaces the recent list and is converted from it at the first start. A vault is added when it has loaded (also one from `--vault` or a second start, never one that failed to load). The start screen lists the names; the window title shows the name of the open vault, with the parent folder after it when two known vaults have the same name.
- **Vault dropdown** (VAULT-030, VAULT-032 to VAULT-034, VAULT-038, VAULT-039, #583): the vault label at the bottom left of the window (name and ▾) opens a list of the known vaults, the open one marked, then **Open…** and **Create…**. A click or Enter opens a vault; arrows, Enter and Esc work. A right-click on a vault offers **Rename** (an empty name is refused). Open… and Create… use the app's own folder picker (a path field, the folders below, Up); Create takes an empty folder (hidden files do not count; a new path is made), can start with sample notes, and offers to open a folder with content instead. A vault whose folder is missing is greyed out in the list and on the start screen with **Locate…** (pick the new folder, the name stays) and **Forget**, and Shale never makes that folder again by itself. A folder inside a known vault, or around one, is allowed and warned about once. A right-click on a vault also offers **Close** (VAULT-035, VAULT-044, #584): the vault is forgotten (its notes stay) and a question asks whether to remove `.shale/` and `.trash/` too (both off by default; there is no system trash, they are deleted for good; notes and `.git` are never touched). **Close current vault** is the last line of the list and the quick switcher has a "Switch vault" command that shows the list. Closing or switching saves all buffers, waits for a running sync without freezing the window (30 s at most, with Cancel), drops everything of the old vault (VAULT-036, VAULT-037) and keeps mail, calendar and the global settings.
- **Opening a vault** (NET-019, #513): a vault is read on a background thread. While it loads, the start screen shows a spinner, "Opening ‹folder›…" and **Cancel** (a vault on a slow network share or an online-only folder no longer freezes the window); the main window appears when the vault has arrived. This covers the last vault at start, `--vault`, a folder a second start hands over, the user guide and Open or Create in the start dialog.
- **Moved so far:** the git sync and the Git LFS setup, the check of the vault's names (Cancel stops it) and the PDF export of a
  folder (Cancel stops it), and the check of the mail index (JOB-015: it starts with the window when a mail account is set up, so the mail view finds the index ready, and the view shows "Checking the mail index…" with a spinner when it is opened earlier). The other long actions follow; the list of all of them, with what each shows today, is
  [docs/spec/busy-states.md](spec/busy-states.md).

## Keyboard

Ctrl is Cmd on macOS. No default key is one that the system reserves (the app menu's Cmd+H, AltGr on Windows, the workspace keys of the Linux desktops), so macOS has other keys for six actions, named in the rows below; a key you set yourself is never refused for that reason. The keys below are the defaults of the shortcuts that can be changed in **Settings → Keybindings** (Change replaces the keys of an action, Add gives it one more, Clear leaves it without a key)
(everywhere, file list, search results, mail view, note view, vim normal mode; a key that another action uses asks whether to overwrite it: Yes takes it from that action, No changes nothing).
Text editing keys and vim keys are fixed.

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Edit / reading view |
| Ctrl+, | Settings |
| Ctrl+Shift+D | Data usage: what Shale keeps on disk, and its caches |
| Ctrl+Shift+PageUp / Ctrl+Shift+PageDown | Previous / next message of the thread of the note (the mails with the same thread-id, by date; nothing happens at the ends, without a thread or in a dialog) |
| Ctrl+Shift+T | Thread overview: opens the Thread view in the right sidebar and gives it the keyboard (↑ ↓ move, Enter jumps, Esc returns to the note) |
| F1 | Open the user guide (built into Shale, unpacked into its data folder and opened as a vault) |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Ctrl+Shift+F | Search notes, mail and posts |
| Ctrl+H / Ctrl+L | Keyboard one place left / right: through the split panes in screen order, left of the first pane into the left sidebar, from there into the first pane (macOS: Cmd+Shift+H / Cmd+Shift+L, because Cmd+H hides the app) |
| Ctrl+PageUp / Ctrl+PageDown, Ctrl+Shift+Tab / Ctrl+Tab | Previous / next tab of the focused pane in the order of the tab bar (from the last tab on to the first), wherever the keyboard is (macOS: Cmd+Opt+← / Cmd+Opt+→ only, Cmd+Tab is the system's) |
| Ctrl+\ | Split right: open this tab in a new pane to the right (panes sit side by side, each with its own tabs) |
| F6 / Shift+F6 | Focus the next / previous pane |
| Ctrl+Shift+W | Close the pane with all its tabs (macOS: Cmd+Shift+W) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k (or ↓ / ↑) next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results (notes, mail and posts): j / k (or ↓ / ↑) next / previous hit, Ctrl+J / Ctrl+K first hit of the next / previous file or group, Enter opens the hit, Esc back to the box |
| Tab / Shift+Tab | In the search over everything (the sidebar search, in its box and while you browse the results, and the picker of Space f g): list only the next / previous source (Notes, Mail, Posts), then the default sources again. Not while the query starts with a prefix (`f:`, `m:`, `s:`). Both can be changed in Settings → Keybindings ("Search results", "List windows") |
| Ctrl+Shift+M | Mail view (its keys are listed below) |
| Alt+← / Alt+→ | Back / forward (macOS: Cmd+[ / Cmd+], because Option+← / → move by word in text) |
| Alt+B, Alt+I, Alt+S, Alt+H, Alt+C, Alt+Y, Alt+F, Alt+Shift+Y, Alt+Shift+F, Alt+E, Alt+U, Alt+O, Alt+T, Alt+Q, Alt+K, Alt+R, Alt+J, Alt+A, Alt+L, Alt+W, Alt+M, Alt+X | The buttons of the formatting toolbar of a note, each doing what a click on it does (the keyboard stays in the text and nothing is typed): Bold, Italic, Strikethrough, Highlight, Inline code, Highlight colour and Font colour (apply the current colour: the main part of the split button; Alt+Shift+Y and Alt+Shift+F open the palette, the ▾ part, where the digits 1 to 8 pick a swatch and 0 picks none, shown as ⇧Y and ⇧F while Alt is held), Heading level (opens its menu), Bullet list, Numbered list, Task, Quote, Code block, Horizontal rule, Table, Link to a note, Web link, Embed, Clear formatting. A key whose button is not there (reading view) or is greyed out (in a table cell) does nothing |
| Ctrl+Shift+K / Ctrl+Shift+J | Fold / unfold the block at the cursor (a heading section, quote, list item or list; again on a folded line folds the block around it); the cursor then goes to the block's first line |
| Ctrl+Shift+O / Ctrl+Shift+I | Fold all / unfold all blocks of the note (also in the reading view) |
| Ctrl+Shift+C | Copy the code of the code block at the cursor (no fences, no newline at the end) |
| Alt+V, Alt+P, Alt+D, Alt+N | The buttons of the view header and tab bar of a note: edit / reading view, Markdown preview, the ⋯ menu (opens it), New tab |
| Alt+1, Alt+2, Alt+3, Alt+4, Alt+5, Alt+6, Alt+7, Alt+8, Alt+9, Alt+0 | The ribbon items by position: the first ten icons of the ribbon, as shown (also after you reordered or removed some) |
| Alt+, / Alt+. | Show or hide the left / right sidebar |
| Hold Alt | A small badge with its key appears at the corner of each of these buttons that has an Alt key (it follows a key you changed). On macOS, where Option+letter types characters, these actions, and the Alt keys of the open mail, have no key until you give them one in Settings → Keybindings (scope Note view, Mail view and Everywhere) |
| Ctrl+6 | Alternate view, in every view (also mail and drawings, where Space is taken): swap with the view this tab showed just before; again: back |
| Space Tab | Vim-style editing, normal mode (not in the mail view, where Ctrl+6 does it): back to the view this tab showed just before (note, calendar, Feeds, …); again: forward to this one |
| Space … | A started key sequence (e.g. `Space f`) shows in the status bar and is dropped after 1.5 s without a key |
| Space f f | In every main view (editor in normal mode, reading view, mail, calendar, Feeds, tasks, graph; not while a text field has the keyboard or a dialog is open; scope "Main views" in Settings → Keybindings): find a file by name (↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens it in this tab, Esc closes) |
| Space f g | In every main view (as Space f f): search the text of notes, mail and posts (`*` and `?` are wildcards; `f:` `m:` `s:` at the start look in notes, mail or posts only; results grouped with counts; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the hit where it belongs, Esc closes) |
| ↓ or Ctrl+J, ↑ or Ctrl+K, Enter, Esc | In a picker window (Space f f, Space f g), the quick switcher (Ctrl+O) and the font list: next line, previous line, open, close. The hint between the search field and the list shows the keys as they are bound now; all four can be changed in Settings → Keybindings ("List windows") |
| Shift+K | Vim-style editing, normal mode: preview the link under the cursor (a note as in the reading view, a web page's title and text; press it again to move the keyboard into the preview: j / k or ↓ / ↑ scroll, Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc closes). Definable, e.g. as Space Space |
| Alt+↑ / Alt+↓ | Move the current line |
| Tab / Shift+Tab | Indent lists or change heading levels |
| Shift+arrows | In a table cell edited in the grid: select a block of cells (formatting buttons and Delete / Backspace work on all of them; typing ends it) |

While a dialog (Settings, an import, a confirmation, a wizard, …) is open, these shortcuts and the editor's keys do nothing; Esc closes the dialog and its own keys (Enter, Tab, typing, the key capture of the Keybindings tab) work. Shortcuts of the system are not affected.

### Mail view keys

In the mail view (Ctrl+Shift+M) one of the three panes (folders, message list, message) has the
keyboard; it has an accent-coloured frame. **Ctrl+H / Ctrl+L** (macOS: Cmd+Shift+H / Cmd+Shift+L) move it one pane left / right (from
the folder pane Ctrl+H goes on into the pane on the left, or the left sidebar, from there Ctrl+L comes back). Letters work
only while no text field has the keyboard. Every key can be changed in Settings → Keybindings
(scope Mail view); Enter and Esc are fixed.

| Keys | Action |
|---|---|
| j / ↓, k / ↑ | Next / previous: folder in the folder pane, message in the list, scroll in the message pane |
| Enter | Folder pane: go to the list; list: open the message (Drafts: open the draft; Outbox: show the mail) |
| Ctrl+J / Ctrl+K | Next / previous folder, from any pane |
| x | Mark the message for a selection (list); Space is the leader of Space f f / Space f g in the mail view too |
| / | Search mail (Esc leaves the field, ↓ or Enter go into the results) |
| t | Search all folders or only this one |
| i | Import to vault |
| u | Mark read or unread |
| s or f | Set or clear the flag |
| d or Delete | Delete (Trash: asks first); in Drafts and Outbox discard the draft or the record |
| Shift+D | Delete the local copy of a mail that was deleted on the server (the dimmed ones; asks first; the server and imported notes are not touched) |
| m | Move to a folder |
| c | Write a new mail |
| r, Shift+R, Shift+F | Reply, reply all, forward |
| Alt+I, Alt+C, Alt+R, Alt+A, Alt+F, Alt+U, Alt+G, Alt+M, Alt+D | The buttons of the open mail: Import to vault, Close, Reply, Reply all, Forward, Mark read / unread, Flag, Move…, Delete (the letter is underlined on the button while Alt is held; not on macOS, where Option+letter types characters: bind them in Settings → Keybindings) |
| Ctrl+R | Sync, or cancel the sync |
| a | Account settings |
| Shift+L | Load the open message (or the list of a folder that is not synced) from the server |
| ] | Select the next attachment (marked with ▸) |
| o, Shift+O | Open the selected attachment, save it to the vault |
| z | Take back a mail waiting in the outbox |
| e | Edit: open the draft, or take a mail of the outbox that was not sent back into the editor |
| Shift+S | Retry a mail of the outbox that was not sent (or send it again when it may have been sent) |
| Menu, Shift+F10 | Open the menu of the message or draft under the cursor (↓ / ↑ or j / k, Enter, Esc) |
| Shift+U | Show or hide the sent mails that are only on this computer (j / k, Enter = Upload again, d = Dismiss, Esc) |
| Esc | Step back: close the message, clear the search, clear the marks, dismiss a message |

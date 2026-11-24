# ADR-0040: Fonts of the exports: any family by name or file, from the vault, the system or TeX Live, with the licence of the font respected

- **Status**: Proposed (2026-11-24; the points of "To confirm" are for the owner)
- **Date**: 2026-11-24
- **Specs**: PDF-034 to PDF-039 ([`docs/spec/features.md`](../spec/features.md)), TEX-054 to TEX-059 ([`docs/spec/tex-export.md`](../spec/tex-export.md)), BOOK-022 ([`docs/spec/book-export.md`](../spec/book-export.md)), CFG-080, CFG-081 ([`docs/spec/vault-and-files.md`](../spec/vault-and-files.md)), JOB-040 ([`docs/spec/engineering.md`](../spec/engineering.md), [`docs/spec/busy-states.md`](../spec/busy-states.md))
- **Issues**: #658 (epic and this ADR); phases 2 to 5 are named in section 9
- **Builds on**: [ADR-0001](0001-spec-driven-development.md) (spec-driven), [ADR-0019](0019-latex-export-managed-texlive.md) (managed TeX Live), [ADR-0039](0039-thesis-citations-and-references.md) (one setting per book, the `bibliography:` rule)

## Context

Today the font of an export is almost fixed. The built-in PDF has three families, chosen by the
note's `print-font: sans | serif | mono` (PDF-014): Ubuntu Light, Liberation Serif and Hack, all
bundled with only their regular face, so bold and italic are imitated by the writer. The LaTeX
export uses what the template set names (Roboto, XCharter, Latin Modern; TEX-005) and has no
setting. A person writing a thesis or a letter has a required typeface (a university's, a
publisher's, Times, Garamond) and cannot say so. #658 asks for that.

Shale already has what is needed: `fontdb` 0.24 (fs and fontconfig) reads the installed fonts,
`skrifa` / `read-fonts` read tables and glyphs, `fontspec` is in the managed TeX Live
(`assets/tex/packages.txt`), and TEX-041 already writes a font file into the work folder.

## Decision

### 1. Keys

Three properties of a note (front matter); the vault settings and the set manifest use the same
three roles:

| Property | Sets | Value |
|---|---|---|
| `print-font` | the text (body, and everything the next two do not take) | a family name (`Libertinus Serif`), a vault path to a font file (`fonts/Garamond.otf`), or `sans`, `serif`, `mono` |
| `print-font-sans` | headings, title, contents, page numbers; in TeX `\sffamily` | a family name or a vault path |
| `print-font-mono` | code (code blocks, inline code, `\ttfamily`) | a family name or a vault path |

- **`sans`, `serif`, `mono` keep their meaning** in the built-in PDF (PDF-014: Ubuntu Light,
  Liberation Serif, Hack). A family really called "Serif" is reached by its file path. In the
  LaTeX export the three words select nothing: the template's own fonts stay (section 7); the
  words name the fonts bundled with the built-in PDF.
- **Headings of the built-in PDF.** Without `print-font-sans`, headings, title, contents and page
  numbers follow `print-font`, as PDF-014 says now. With it, they use that font and the body keeps
  `print-font`. In TeX the templates already separate sans from the main font, so
  `print-font-sans` changes the template's sans (`\setsansfont`) and `print-font` alone changes
  only the main font. For `print-font` alone the two engines therefore differ where a template
  sets its headings in sans; that is the template's design, not a bug.
- **Code and hand-drawn text.** Without `print-font-mono`, code stays Hack (PDF) or the
  template's mono (TeX). Virgil (hand-drawn text) and STIX Two Math (math, PDF-012) are not
  configurable.
- A value is one family. A list ("Garamond, Georgia") is not supported: the fallback is the
  order of sources in section 3 and the next entry of the precedence, not a CSS font stack.
- **Bad values.** An empty value is a missing one. A value over 200 characters, with a control
  character, or (as a path) outside the vault or not a font file, is ignored with a warning
  (section 5), like the other `print-*` properties (PDF-013).

The vault defaults are three fields in Settings, Vault, "Fonts of the exports" (`print_font`,
`print_font_sans`, `print_font_mono` in `<vault>/.shale/settings.json`, default empty and left out
of the file while empty, a new field with a default so no version bump, CFG-006). A template set
has the same three as manifest keys (`font`, `font_sans`, `font_mono` in its `settings.toml`),
beside `table_caption` and `figure_caption` of TEX-051.

### 2. Precedence

Each of the three keys is decided on its own; the first entry that gives a usable value wins:

1. the note's property (for a book: the top folder note's, entry 2);
2. **a book's top folder note**: a book has one font. A chapter note's own `print-font*` is
   ignored with a warning, as `bibliography:` is (BOOK-021);
3. the template set's key (**LaTeX only**: the built-in PDF has no set);
4. the vault setting;
5. the built-in default (PDF: Ubuntu Light and Hack; TeX: what the template says).

A value that cannot be used (sections 5 and 6) falls to the next entry of this list, not straight
to the default, so a bad note property still gets the vault's font.

### 3. Where a font comes from

In this order, the first source that has the family:

1. **A font file in the vault.** The folder `fonts/` at the vault root, any depth below it (a
   dot-folder is not searched). Files: `.ttf`, `.otf`, `.ttc` (a collection counts as its faces).
   For the LaTeX export the **template set's own folder** `templates/tex/<set>/fonts/` counts
   too, searched after the vault's `fonts/`, so a set can carry the typeface its institute
   requires (a set is already synced with the vault, TEX-005). The built-in PDF uses only the
   vault's `fonts/` (it has no set).
2. **A system font**, through `fontdb` (the scan of section 8).
3. **A TeX Live font, for the LaTeX export only.** The name is passed to `fontspec`, which
   `luaotfload` resolves from the managed TeX Live's fonts (`Libertinus`, `TeX Gyre Pagella`).
   The built-in PDF has no such source: a name it does not find in 1 and 2 is a missing font
   (section 5).

**Naming.** Either the family name as the font states it (name table ID 16 "typographic family"
first, then ID 1), or a vault path to one file (`fonts/Garamond-Regular.otf`, also as a wiki link
`[[Garamond-Regular.otf]]`, found as `logo:` is, BOOK-019). A family name matches
case-insensitively and ignoring runs of spaces, `-` and `_` (`libertinus serif`,
`Libertinus-Serif`); there is no prefix or fuzzy match, so a typo is a missing font, not another
font. Where a vault file and a system font have the same family, the vault wins (entry 1) and the
faces of the two are never mixed.

**Faces.** A family gives its four faces: regular (weight 400, upright), bold (the nearest weight
from 600 to 900, preferring 700), italic (weight 400, italic or oblique) and bold italic;
`fontdb`'s matching does the picking. A **path** names one file and gives **only that face**: it
is the regular face, and bold and italic of that text are the fallback of section 4 (in TeX,
`fontspec` cannot find them either and does the same). A **variable font** is used at its default
instance (named instances are not read); its bold is the fallback of section 4.

### 4. Faces in the built-in PDF

When the family has a bold, an italic or a bold-italic face, **that face is embedded and used**
for `**bold**`, `*italic*`, headings and the like, instead of the writer's imitation (shear and
stroke). The imitation stays only as the **fallback for a missing face**, and the export then
says so in a warning ("Garamond has no bold face: bold text is imitated", once per face per
export). The bundled fonts keep today's behaviour (PDF-014), as they have only their regular face.

The fallback chain for characters the font lacks (PDF-010) stays: the chosen font, then the
bundled ones in the order PDF-010 gives (Ubuntu Light, Hack, the emoji font, Liberation Serif),
so a font without arrows or emoji does not turn them into boxes. A font is embedded **subsetted**
and only when the PDF uses it (as Liberation Serif is, PDF-014), unless its licence says
otherwise (section 6).

### 5. A missing font

Never a failed export. A font that is not found, cannot be read (not a font, damaged) or may not
be embedded (section 6) is **a warning with the key, the value and what is used instead**
("`print-font`: the font "Garamond" was not found; using Ubuntu Light."), and the next entry of
the precedence of section 2, then the default, is used. The warnings are those the export already
shows for a template the vault lacks (TEX-033) or a chapter's own bibliography (BOOK-021): the
dialog's list and the report after the export. In the LaTeX export a name found neither in the
vault nor by the system is passed to TeX Live (section 3.3), and section 7 says what happens when
that fails.

### 6. Licences: the OS/2 `fsType` bits

A font file says in its OS/2 table's `fsType` field how it may be embedded in a document (the
OpenType specification, "OS/2 and Windows Metrics Table", field `fsType`, under "Embedding
licensing rights"; the same meaning in TrueType). Shale reads it with `read-fonts` before it uses
any font **file**, a vault file or a system font. The fonts of TeX Live are free licences and the
bundled fonts have licence files in `assets/fonts/`, so neither is checked at run time.

| Bits | Value | Name in the specification | Shale |
|---|---|---|---|
| none | 0 | Installable embedding | used |
| 1 | 0x0002 | Restricted License embedding: must not be embedded | **not used**: warning, section 5 |
| 2 | 0x0004 | Preview and Print embedding | used (a PDF is for print and view) |
| 3 | 0x0008 | Editable embedding | used |
| 8 | 0x0100 | No subsetting | used, but **embedded whole**, not as a subset |
| 9 | 0x0200 | Bitmap embedding only | **not used** (Shale embeds outlines, not bitmaps): warning, section 5 |
| 0, 4 to 7, 10 to 15 | | reserved | ignored |

- The specification says that bits 1, 2 and 3 are mutually exclusive in a table of version 3 or
  later and that, when several are set, the least restrictive applies; Shale does the same
  (restricted counts only when 0x0002 is the **only** one of these three).
- **A font without an OS/2 table counts as installable**, as does one whose `fsType` cannot be
  read.
- The vault and the system are treated alike: having a font file is not a licence to embed it.
- The check is Shale's reading of one field, not legal advice: the licence text of a font can say
  more, and the owner of the document answers for it (a font in the vault's `fonts/` has its
  licence file beside it, where it can be seen).
- The LaTeX export applies the check to vault and system fonts too, before it writes the override
  (LuaTeX itself ignores `fsType`), so a font the PDF refuses is refused there.

### 7. The LaTeX export

- **Where.** `\setmainfont`, `\setsansfont` and `\setmonofont` are written in the helper block of
  `writer::helpers`, **after the class and the template's setup** (`class_and_preamble`), so they
  override what the template says, and before `\begin{document}`. Only the keys that have a value
  are written; a document without any is what it is today.
- **A vault font file** (or one of the set's `fonts/`) is **copied into the work folder** beside
  `shale-emoji.ttf` and named with `Path=./` and its faces:
  `\setmainfont{Garamond-Regular.otf}[Path=./, BoldFont=Garamond-Bold.otf, ItalicFont=...,
  BoldItalicFont=...]`. The faces are those of section 3 found in the **same family** among the
  files; a face that does not exist is left out, and `fontspec` does what it does for a missing
  face. A font file is copied once and only when used.
- **A system font** is passed **by name** (`\setmainfont{Garamond}`), not copied: `luaotfload`
  scans the system fonts itself. A **name found nowhere by Shale** is passed by name too
  (section 3.3).
- **The emoji font keeps working** (TEX-041): `\shaleemojiface` is its own `\newfontface` with
  `Path=./`, independent of `\setmainfont`, so no override touches it. The helpers keep their
  order (fontspec loaded, then the emoji face, then the overrides), and the emoji of a document
  with a font override are still drawn by Noto Emoji. A test covers it (TEX-056).
- **A class that is not based on fontspec.** The writer already loads `fontspec` when the class
  has not (`\IfPackageLoadedTF`, TEX-041), and the engine is always LuaLaTeX (ADR-0019), so
  fontspec works with a class that loads a font package (`mathpazo`, `libertine`) or sets
  `\rmdefault`: `\setmainfont` after it wins for the text and leaves the maths of such a class
  alone. A class that cannot be combined with fontspec at all (it stops with an error when
  fontspec is loaded) fails the compile like any template error; that is the template's problem
  and the retry below does not help it.
- **A font TeX cannot find** (a name that is neither in the vault nor in the system, and that
  `luaotfload` does not know): `fontspec` stops with "The font ... cannot be found". Shale does
  **not** check beforehand: `luaotfload-tool --find` loads the font database, a start of its own
  for every export, and the database can be stale or being rebuilt. It reads the error after the
  compile (the log parser of `compile.rs` already recognises it), **warns** ("the font "X" was
  not found by TeX; the template's font is used"), removes the overrides of the missing font(s)
  and **compiles once more** without them. One retry, only for this error; the overrides of the
  other keys stay. The result is a PDF with a warning, not a failed export.

### 8. The system-font scan

`fontdb`'s scan of the system fonts can take seconds on a machine with thousands of fonts. It
runs **in a background job through the busy registry** (JOB-040: "Looking up fonts...", quiet in
the sense of JOB-022, so listed only after 2 s, no Cancel since it writes nothing). Its result,
the database of faces (family, style, weight, path, `fsType`), is **cached for the session** and
rebuilt only when asked (a button "Look up fonts again" beside the fields of the settings) and
for the vault's own `fonts/` when that folder's index entries change (as the bibliography is,
JOB-039). The export waits for the scan when the font it needs is not yet known and the scan runs;
it never starts a second scan (JOB-005). The scan starts when the export dialog or Settings first
needs the list, so the export usually finds it ready, and by the export itself otherwise. A
settled window waits for it (DEV-041).

### 9. Phases and overlaps

1. **Phase 1**: this ADR and the spec rows (no code).
2. **Phase 2**: the resolver: the keys and precedence, the sources, the name matching, the faces,
   `fsType`, the scan job. Tested on its own; no export uses it yet.
3. **Phase 3**: the built-in PDF uses it: real faces, `print-font-sans`, the warnings.
4. **Phase 4**: the TeX writer: the overrides, the copy of a vault font, the retry, the podman
   `tex_env` test with a vault font.
5. **Phase 5**: the export dialog (PDF-019 shows the choice), Settings, the user guide and its
   screenshot.

Phases 3 and 4 come **before #648 and #647**, which change the same files (`pdf/export.rs`,
`pdf/write.rs`, `pdf/tex/writer.rs`). **#653** (the KOMA-Script set) uses the set default of
section 1 for its own fonts, so it comes after phase 4 for that key. #644 and #645 do not touch
fonts. The thesis of ADR-0039 is one of the places where a required typeface is set, which is why
the one font per book (BOOK-022) is decided here.

### 10. Alternatives considered

- **Bundle more fonts** (Liberation Sans, a Times-like serif with bold and italic): fixes the
  look of three families but not the university's own font, and every font costs binary size and
  a licence file. Rejected for the question asked; a bundled bold and italic of Liberation Serif
  stays possible later without a change of keys.
- **Only vault fonts** (no system scan): simple and reproducible (the vault carries the font, like
  the TeX templates, TEX-005), but a person who has Garamond installed must copy it into the
  vault. Rejected; the vault wins by precedence, so a book that must be reproducible can still
  pin its font.
- **Only system fonts**: the export would differ from machine to machine and a synced vault would
  break on another computer. The vault source exists for that.
- **Let TeX Live resolve everything**: the built-in PDF would have nothing.
- **A CSS-like list** (`print-font: Garamond, Georgia, serif`): more to specify and to explain;
  the warning and the next entry of the precedence do the useful part.
- **A check with `luaotfload-tool --find` before the compile**: rejected in section 7 for the
  start-up cost and a stale database; the retry costs one compile only when it happens.
- **No `fsType` check**: Shale is the tool that embeds the font into a file that is handed on;
  the standard's bits are cheap to read, and a warning with a fallback is friendlier than a file
  that breaks a licence. It is not a legal service (section 6).
- **Imitated faces for good**: the present state; real faces are the point of #658.
- **A separate `print-heading-font`**: the sans and mono pair matches `fontspec` and the
  templates' own split (main, sans, mono) and gives the PDF the same three roles.

## Consequences

- One resolver feeds the built-in PDF, the LaTeX export and the dialog, so the precedence, the
  sources and the licence rules exist once.
- A vault with a `fonts/` folder is self-contained (synced, in the backups); the fonts in it are
  files the owner of the vault answers for.
- The built-in PDF embeds up to four faces per role, subsetted and only those the document uses.
- Existing notes do not change: no property, no setting and no `fonts/` means today's output.
- The font scan is one more task of the busy registry (JOB-040).

## To confirm

- Section 3: the folder `fonts/` at the vault root; that a template set's `fonts/` counts for TeX
  after it; that a system font is searched before TeX Live; that a path gives only one face.
- Section 6: restricted (0x0002) and bitmap-only (0x0200) are refused; 0, 0x0004 and 0x0008 are
  allowed; 0x0100 is embedded whole; no OS/2 table is installable; of bits 1 to 3 the least
  restrictive wins, as the specification says.
- Section 7: one retry after the "font cannot be found" error, without the override, instead of a
  check with `luaotfload-tool --find`; `sans|serif|mono` select nothing in TeX; a system font is
  passed by name and not copied.

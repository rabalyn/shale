# ADR-0039: Thesis writing: Pandoc citations, labelled figures, tables, equations and sections, a BibTeX bibliography, and three engines

- **Status**: Proposed (owner decisions of 2026-11-24 are in; the points marked "to confirm" below are the writer's and wait for the owner)
- **Date**: 2026-11-24
- **Specs**: TEX-043 to TEX-051 ([`docs/spec/tex-export.md`](../spec/tex-export.md)), BOOK-020, BOOK-021 ([`docs/spec/book-export.md`](../spec/book-export.md)), EDIT-122 to EDIT-129 ([`docs/spec/links-and-editor.md`](../spec/links-and-editor.md)), PDF-032, PDF-033 ([`docs/spec/features.md`](../spec/features.md)), SITE-077 ([`docs/spec/site-export.md`](../spec/site-export.md)), DOCX-014 ([`docs/spec/word-export.md`](../spec/word-export.md))
- **Issues**: #642 (epic), #643 (this ADR); phases #644 to #654
- **Builds on**: [ADR-0019](0019-latex-export-managed-texlive.md) (managed TeX Live), [ADR-0001](0001-spec-driven-development.md) (spec-driven)

## Context

The owner wants Shale to be the text processor for a bachelor's or master's thesis (#642). In the
LaTeX export first, then in the built-in PDF, the site and the Word export, a thesis needs:
citations with a bibliography, numbered figures and tables with captions, numbered equations,
references to all of them and to sections, front matter (abstract, declaration, roman page
numbers, appendix chapters), a list of abbreviations, lists of figures and tables, and a
KOMA-Script template besides TUDa-CI.

What exists: math (EDIT-046, EDIT-047, TEX-029, PDF-012, SITE-028), captions under embeds from the
link's alias without numbers (EDIT-053, PDF-004, TEX-015), footnotes (EDIT-050), numbered headings
(LINK-014), the book export (a thesis is a book: chapters as notes, BOOK-*), the managed TeX Live
with `biber` already in `assets/tex/packages.txt` (TEX-002), and the Word export through pandoc
(DOCX-*).

The syntax must be decided once, because five writers read it (the editor, the TeX writer, the
built-in PDF, the site, the Word export through pandoc), and because `@` already appears in
people's notes as e-mail addresses, in mail notes, and in posts of the feeds.

## Decision

### 1. Citations: Pandoc's syntax

| Written | Meaning | TeX (biblatex) |
|---|---|---|
| `[@key]` | parenthetical citation | `\autocite{key}` |
| `[@key, p. 12]` | with a locator | `\autocite[p. 12]{key}` |
| `[see @key, p. 12]` | prefix and locator | `\autocite[see][p. 12]{key}` |
| `[@a; @b]` | several | `\autocites{a}{b}` (with their own prefixes and locators) |
| `@key` | in the text, the author is the subject | `\textcite{key}` |
| `@key [p. 12]` | the same with a locator | `\textcite[p. 12]{key}` |
| `[-@key]` | the author suppressed | `\autocite*{key}` |

The key grammar is Pandoc's, narrowed: it starts with a letter, digit or `_`, and goes on with
letters, digits and `_`, and the internal punctuation `: . - + /` where a letter, digit or `_`
follows (so a full stop at the end of a sentence is not part of the key). A key outside this
grammar cannot be cited (the key index says so).

### 2. Labels: pandoc-crossref's syntax

A label is `{#kind:id}` with the kind `fig`, `tbl`, `eq` or `sec` and an id of letters, digits,
`_`, `-`, `.` and `:` (so `{#fig:results}`). Where it attaches:

- **Figure**: directly after an embed that stands on a line of its own, with no or one space:
  `![[plot.png|Results of run 2]]{#fig:results}`. The alias is the caption (EDIT-053); the figure
  is numbered when it has a label. An embed without a label stays as it is today (an unnumbered
  caption), so no existing note changes.
- **Table**: a Markdown pipe table (EDIT-024) with, on the line directly below it, a caption line
  `: The caption {#tbl:runs}` (Pandoc's table caption). A table without such a line is not
  numbered.
- **Equation**: after the closing `$$` of display math, on the same line, one space or none:
  `$$ E = mc^2 $$ {#eq:energy}` or `$$` lines followed by `$$ {#eq:energy}`; on a ```` ```math ````
  block, in the info string: ```` ```math {#eq:energy} ````. A numbered display has one number, also
  when it has several lines (`aligned`, `cases`, matrices).
- **Section**: at the end of a heading line: `## Method {#sec:method}`.

A label is unique in its document (a book: in the whole book). A second use is marked and is a
warning of the export; the first one wins.

### 3. References: `@kind:id`

`@fig:results`, `@tbl:runs`, `@eq:energy`, `@sec:method` give the kind's word in the document's
language and the number, as `\cref` does: "Abbildung 3" / "Figure 3", "Tabelle 2" / "Table 2",
"Gleichung (2.1)" / "Equation (2.1)", "Abschnitt 3.2" / "Section 3.2" ("Kapitel" / "Chapter" for
the chapter level of a book, "Anhang A" / "Appendix A" for an appendix chapter). Several in one
bracket, `[@fig:a; @fig:b]`, give "Abbildungen 1 und 2" / "Figures 1 and 2". `[-@fig:results]` is
the number alone ("3", "(2.1)"). Numbers are drawn, never written into the file (as LINK-014 does).
The reference words exist in German and English; any other language uses English.

The **document's language** is, in this order: the property `language:` of the note (of a book:
of the top folder note), the `language` of the template set's settings (TEX-005), English.
(To confirm: the order, and a property named `language`.)

Captions read "Abbildung 3: …" / "Figure 3: …". Tables carry their caption above, figures below
(the usual order of KOMA-Script; the template may change it). Numbering is **continuous** in a
single note, **per chapter** (3.1, 3.2) in a book, and the property `float-numbering:` of the
note or top folder note (`continuous` or `chapter`) overrides either. It counts figures, tables
and equations alike. (To confirm: per chapter as the default of a book.)

### 4. The clash with `@`, as rules

`@` has no meaning in a note today: the editor and the Markdown scanners (`src/markdown.rs`) read
`[[…]]`, `#tags`, `[^footnotes]`, `$math$` and `<tags>`, nothing with `@`. It appears as e-mail
addresses (`ada@host.org`, `<ada@host.org>`, `mailto:`), in the notes of the mail import
(`type: email`) and the feeds (`type: social-post`, text with `@user@host` and `@name.bsky.social`
mentions, SOC-*), and as plain words (`@todo`). The rules, applied by one scanner for all writers:

1. **Where**: only in prose. Never in front matter, fenced or inline code, math, the target of a
   link, a URL or an autolink, an HTML tag, a `[[wikilink]]`, and never in a note whose `type:` is
   `email` or `social-post` (the text is somebody else's).
2. **Word boundary**: an `@` starts a citation or reference only at the start of a line or after
   whitespace, `(`, `[`, `;`, `,`, or an opening quote; and `-@` only as `[-@`. After a letter,
   digit, `_`, `.`, `-`, `+` or `/` it never does. So `ada@host.org` is no citation.
3. **A mention is no citation**: a key that is followed directly by another `@` and a letter or
   digit (`@ada@mastodon.social`) is plain text as a whole.
4. **A bare `@key`** (not in brackets, not a reference) is a citation only when a bibliography is
   in effect (§5) and holds the key (case-sensitive). Otherwise it is plain text, with no mark:
   `@ada.bsky.social`, `@here` and `@todo` stay what they are.
5. **A bracketed group** `[@…]`, `[-@…]`, `[text @key…]` is a citation group when one of its
   `;`-separated items has `@key` at its start or after prefix text. It is no group when `(`,
   `[` or `:` follows the closing `]` (a link, a reference link, a definition: `[@a](url)`).
   With a bibliography in effect a key it lacks is marked as an unknown key, like a broken link
   (EDIT-126). Without a bibliography the group is plain text; the TeX export then says that the
   note has citations and no bibliography (TEX-044).
6. **Reserved prefixes**: `fig:`, `tbl:`, `eq:`, `sec:` (references, §3) and `abbr:`, `gls:`
   (§8) always make a reference, bibliography or not. A reference to a label that does not exist
   is marked like a broken link. A `.bib` key that starts with a reserved prefix is not citable;
   the key index says so.
7. **Labels** `{#kind:id}` are read only in the places of §2. Any other `{#…}`, and a `#` after
   `{`, is plain text and no `#tag` (the tag rule needs a space, a tab, `(` or the line start
   before the `#`, so there is no clash).
8. **Escape**: `\@` is a plain `@` (a CommonMark escape already). Nothing else is needed.

### 5. The bibliography

- **Format**: a BibLaTeX/BibTeX `.bib` file (owner decision), not CSL-JSON. The export of Zotero
  with Better BibTeX, of JabRef and of Citavi is read.
- **Which file**: the property `bibliography:` of the note, or of a book's top folder note, found
  as a picture is by `logo:` (BOOK-019: a vault path, a file name or a wiki link); a list names
  several. A vault default, a setting in Settings, applies when the note has none. In a book
  only the top folder note's property and the default count (one bibliography per book; a chapter
  that names another file is a warning). A file that is missing or does not parse means "no
  bibliography in effect" plus a message.
- **Placement**: at the end of the document, as an unnumbered chapter headed "Literaturverzeichnis" /
  "References" (by the document's language; in the table of contents). A line of its own, the
  marker `<!-- shale: bibliography -->`, puts it there instead; the same form is the marker of
  `list-of-figures`, `list-of-tables` and `list-of-abbreviations` (§8). An HTML comment shows in
  no other Markdown program and is no link, tag or mention.
- **Style**: five named styles, `numeric`, `authoryear`, `alphabetic`, `ieee`, `apa`. Each is a
  biblatex style in the TeX export and a CSL file shipped with Shale in the built-in engine
  (§6). Chosen in this order: the export dialog, the property `citation-style:`, the template
  set's default (`settings.toml`), then `authoryear`. (To confirm: `authoryear` as the default.)
- **Lists of figures and tables**: boxes of the export dialog, or the markers above.

### 6. Three engines

| Export | Engine | Why |
|---|---|---|
| TeX | **biblatex with biber**, and `cleveref`, `caption`, `glossaries-extra` | the standard of theses; biber is in the managed TeX Live. TEX-004's compile becomes multi-pass: LuaLaTeX, biber when the run wrote citations, LuaLaTeX until labels are stable (at most 4 runs). (#644) |
| Built-in PDF and site | **the `hayagriva` crate** (Rust, MIT or Apache-2.0, used by Typst), reading the `.bib` and formatting with CSL | no TeX needed. **Confirmed in #645**: its licence, its dependencies, and how well it reads real exports of Zotero, JabRef and Citavi. **If it fails that evaluation**: the `biblatex` crate for reading plus `citationberg` for the CSL styles (both Typst's, the parts hayagriva is made of), and as the last resort a small reader of our own with the five styles written by hand. The syntax and the numbering do not change. |
| Word | **pandoc `--citeproc`** with the same `.bib` and the CSL file of the chosen style, on top of #487 | pandoc is already the user's program (BUILD-018). Cross-references through Shale's own Lua filter (DOCX-004 to DOCX-006), as pandoc-crossref would be a second program to install; confirmed in #650. |

The editor and the reading view use the same reader and index as the built-in export (#645, #646).

### 7. Front matter and appendix

A note of a book is marked by the property `thesis-part:` with `abstract`, `declaration`
(the "Eidesstattliche Erklärung"), `preface` or `appendix`; in a single note a heading takes the
class after it: `# Abstract {.abstract}`, `# Declaration {.declaration}`, `# Appendix {.appendix}`
(Pandoc's attribute form, next to a label). Shale writes no wording of the declaration; it is
the note's text. The notes before the first note without `thesis-part:` (the first chapter) are
the front matter: roman page numbers, headings unnumbered, in the table of contents; page 1 and
arabic numbers start at chapter 1. Notes with `thesis-part: appendix` come after the chapters, as
chapters lettered A, B, ….

### 8. Abbreviations and glossary

Two fenced blocks, anywhere in the book (usually one note of its own):

````
```abbreviations
cpu: CPU | Central Processing Unit
ml: ML | Machine Learning
```
```glossary
entropy: Entropy | A measure of disorder
```
````

An abbreviation line is `key: short | long`, a glossary line `key: name | description`. In text,
`@abbr:ml` writes the long form with the short in brackets at its first use ("Machine Learning
(ML)"), the short form after that; `[-@abbr:ml]` is always the short form; `@gls:entropy` is the
name. The marker `<!-- shale: list-of-abbreviations -->` prints the list, and `list-of-glossary`
the glossary (the TeX export: `glossaries-extra` with `\makenoidxglossaries`, which needs no extra
program; confirmed in #652, fallback `acro`).

## Alternatives considered

- **CSL-JSON as the bibliography**: Zotero and Pandoc read it, but the owner chose BibLaTeX only;
  one format means one reader, one completion and one TeX path (biber reads `.bib` itself).
- **Wiki-link style citations (`[[@key]]`)**: would fit Shale's links, but nobody else reads
  it; Pandoc's syntax already works with the Word export and with Obsidian's citation plugins.
- **`\cite` commands typed in the note**: works in TeX only and shows in the editor as noise.
- **`@` as a reference without a prefix** (`@results`): clashes with bare citations and
  mentions; the reserved prefixes avoid a lookup of both tables to decide.
- **Numbering any embed with a caption**: would renumber every existing note; a label is the
  opt-in.
- **Labels in a separate property or sidecar**: keeps the text clean but loses the place a label
  belongs to and breaks when the figure moves.
- **`glossaries` with `makeglossaries`, or `bib2gls`**: needs another program in the compile;
  `\makenoidxglossaries` does not.
- **Only one engine (TeX for all)**: the built-in export and the site must work without TeX
  (ADR-0019 keeps TeX optional).

## Consequences

- One scanner (`markdown` module, EDIT-122) is the only reader of the syntax; TeX writer,
  built-in PDF, site and the Word filter take its result, so they agree on numbers.
- TEX-004 changes from one run to several; the timeout and the kill rule apply to each pass.
- Existing notes do not change: nothing is recognised without a label, a bibliography, or a
  reserved prefix.
- The Word export's filter has to number as the other engines do; the shared rules (§3) are
  tested in each engine against the same sample.
- Points to confirm by the owner: the document language order, per-chapter numbering as the
  default of a book, `authoryear` as the default style, the property names (`bibliography`,
  `citation-style`, `float-numbering`, `thesis-part`, `language`), the marker syntax
  (`<!-- shale: … -->`), the `abbreviations` / `glossary` blocks and the `abbr:` / `gls:` prefixes,
  and that a figure needs a label to be numbered.

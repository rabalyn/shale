# ADR-0041: Citations and cross-references in the Word export: pandoc `--citeproc` formats, Shale numbers and resolves

- **Status**: Accepted (2026-11-25; #650, within the choices of [ADR-0039](0039-thesis-citations-and-references.md) §6, which named pandoc `--citeproc` and Shale's own filter)
- **Date**: 2026-11-25
- **Specs**: DOCX-014 to DOCX-017 ([`docs/spec/word-export.md`](../spec/word-export.md))
- **Issues**: #650 (this change), #642 (epic)
- **Builds on**: [ADR-0039](0039-thesis-citations-and-references.md) (the syntax, one scanner, the numbering rules), [ADR-0001](0001-spec-driven-development.md)

## Context

The Word export (DOCX-001 to DOCX-013) runs the user's pandoc on a note's Markdown: pandoc reads
it with the **commonmark** reader and Shale's Lua filter, writes the docx, and Shale finishes the
file. The thesis syntax of ADR-0039 has to come out of it with the same results as the TeX export
(TEX-044 to TEX-048) and the built-in PDF (PDF-032): citations with a bibliography, numbered
figures, tables and equations with captions, and references such as "Abbildung 3" / "Figure 3"
in the language of the note.

Two facts about pandoc decide the shape of it:

1. **Citations.** pandoc formats them itself, with `--citeproc` and a CSL file (built in since
   pandoc 2.11), so the five named styles of ADR-0039 §5 and the style files people already have
   work, and a Word reader sees what any pandoc user would. But the commonmark reader has **no
   `citations` extension** (`pandoc --list-extensions=commonmark`, 3.11): `[@key]` stays text. The
   `markdown` reader has it, and so has `commonmark_x`, but switching readers changes how every
   note is read (DOCX-002: `==mark==`, wikilinks, `[[…]]` embeds, tables, task lists), which the
   whole Word export and its round trip with the import (DOCX-010) rest on.
2. **Cross-references.** pandoc has none of its own.

## Options for the cross-references

| | pandoc-crossref | A Lua filter that numbers and resolves | Shale resolves, then pandoc reads |
|---|---|---|---|
| Needs | a **second program**, `pandoc-crossref`, a Haskell binary that is built for **one exact pandoc version** (it refuses to run on another) and is not part of pandoc itself (a separate package or download; not installed on the development computer) | nothing; the filter is built into Shale (as DOCX-004's is) | nothing |
| Syntax | pandoc's attributes `{#fig:x}` and `@fig:x` on the `markdown` reader | the filter would read the thesis syntax again | the scanner of EDIT-122, already used by the editor, the TeX writer and the built-in PDF and site |
| Numbers | its own counters; per chapter needs `chapters: true` and numbered sections; `eq:` numbers need display math in a certain form | a third implementation of EDIT-128 in Lua, which has to agree with the Rust one | **the same numbers**: `labels::of_note` (EDIT-128): continuous or per chapter, `float-numbering:`, the section numbers of LINK-014 |
| Words | `figureTitle`, `tableTitle`, `eqnPrefix` and so on as metadata, which Shale would have to fill in German and English (the words of ADR-0039 §3, again) | the same | `thesis::reference`, `thesis::group` and `View::caption_text`, the words and the grouping ("Abbildungen 1 und 2", "Gleichung (2.1)", `[-@fig:x]`) of every other export |
| Errors | warnings on pandoc's stderr, in its words | in Lua | the warnings of `View::warnings` (duplicate labels, a label nobody has), as in the TeX export and the PDF |
| Test | only with both programs | only with pandoc | **the text pandoc is given is a unit test** that needs no pandoc at all; pandoc is needed only to see the Word file |

## Decision

**Shale resolves the cross-references and the numbers in Rust, with the model of `src/thesis.rs`
and the one scanner, and writes the result into the text pandoc reads; pandoc formats the
citations.** pandoc-crossref is not used and not needed.

- A labelled figure, table or equation, and a labelled heading, get their number and caption as
  `View` gives them; a reference `@fig:x` (and `[@fig:a; @fig:b]`, `[-@fig:a]`) is the text of
  `View::reference_text`. They are written as **marks** in the text: a private-use character, a
  number and a closing mark, which stand for words in a small Lua file next to the filter (so no
  character of a caption or of a section title can be taken for Markdown). The filter turns the
  mark of a reference into its words, and the mark of a caption into a paragraph of Word's
  *Image Caption* or *Table Caption* style, above or below the figure or table as the properties
  `figure-caption:` and `table-caption:` say.
- A **citation group** is a mark too, with the data of its keys, modes (`[@a]` normal, `[-@a]`
  author suppressed, `@a` in the text), prefixes and locators. The filter makes pandoc's own
  `Cite` of it, and `--citeproc`, which runs after the filter, formats it with the `.bib` file
  and the CSL file. The reader stays commonmark, so nothing else about the note changes.
- **The style** is one of the five CSL styles that the built-in exports format with (hayagriva's
  archive, `Style::csl_xml`), so Word shows what the PDF shows, unless the property
  `citation-style:` names a `.csl` file of the vault, which is used as it is. pandoc's default
  style is not relied on.
- **The bibliography** is `# Literaturverzeichnis` or `# References` and pandoc's `refs` div,
  at the end or at `<!-- shale: bibliography -->`, when the note cites a key.
- A section number is written in front of the title of a **labelled** heading ("1.1 Method"),
  because Word's headings are not numbered by the export and a reference to "Section 1.1" has to
  lead to something readable. Unlabelled headings stay as they are.
- An equation's number is `\qquad (N)` at the end of its formula: Word's equation editor has no
  right-margin tag that pandoc writes. A labelled ```` ```math ```` block is written as a `$$`
  block (pandoc has no math fence).
- Numbers are text, not Word fields (`SEQ`), and references are not links or cross-reference
  fields: Word cannot update them, but they are right in the file the person gets. Fields are a
  possible later step on top of the same marks (a bookmark at the caption, a `REF`).

## Alternatives considered

- **pandoc-crossref**: see the table. A thesis writer who has it could use it, but Shale must not
  need a program the person may not have, and two sets of numbers (Shale's in the editor, the
  PDF and the TeX; its own in Word) would disagree in a thesis with chapters and appendices.
- **The `markdown` or `commonmark_x` reader** to get `citations`: changes the reading of every
  note; see Context.
- **A Lua filter that finds `@key` in the text** after the commonmark reader: it would read the
  syntax a second time and could disagree with the scanner on the cases of ADR-0039 §4 (an e-mail
  address, `@todo`, a mention, a link). The scanner decides once, in Rust.
- **Writing the formatted citations in Rust** (hayagriva) and giving pandoc plain text: then the
  Word file has no `--citeproc` and no CSL, which the issue asks for, and the text in the file
  could not be re-styled by a person who opens the `.docx` and the `.bib` with pandoc.

## Consequences

- One more way for the text pandoc reads to differ from the note: the marks. They are private-use
  characters; a note that contains them itself would have them taken for marks only when the
  data file has the number, so they are harmless otherwise (and a note without the thesis syntax
  gets no data file at all, so nothing changes for it).
- pandoc must be 2.11 or newer for `--citeproc`; an older one makes the export say so (DOCX-017).
- The lists of figures and tables of the other exports are not made in Word (a Word contents of
  figures needs fields). Left open in #650.
- A book (BOOK-*, #649) takes the same preparation for each of its notes when it is exported to
  Word; this ADR covers one note.

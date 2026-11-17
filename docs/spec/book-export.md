# Spec: a folder as one PDF (BOOK)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Issue: #306.

A folder of the vault, up to the vault itself, is exported as **one PDF**, a book: its markdown
notes in a fixed reading order, one after the other, with a title page, one table of contents
and running headers. The built-in engine (PDF-001…PDF-008, `src/pdf/`) writes it; every note is
laid out as its single export lays it out (embeds, tables, code, colours). Tests are named
`file.rs::test_fn` (under `src/`).

## The folder convention

How to organise a folder so that it prints as a consistent book. The convention needs no
renaming: links keep working and the folder stays an ordinary vault.

```
Handbook/                     ← the book (title: the folder note's `title`, else "Handbook")
  index.md                    ← folder note: introduction, title page data, the reading order
  Getting started.md          ← chapter 1 (index.md links it first)
  Writing/                    ← part 2, a folder
    Writing.md                ← its folder note (named like the folder)
    Lists.md                  ← 2.1
    Tables.md                 ← 2.2
  Appendix.md                 ← 3 (not linked: after the linked ones, by name)
  Draft.md                    ← `export: false`: left out
  z_media/                    ← no notes: not a part (its pictures come in through embeds)
```

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOOK-001 | **Chapters are notes.** Every `.md` note in the folder and its subfolders is a chapter, except a note whose front matter has `export: false`. Other files (pictures, drawings, PDFs, sounds, videos) are no chapters; they are printed where a note embeds them (PDF-004, PDF-005). Folders whose name starts with `.` or `_`, the folders `exports/` and `templates/` of the vault root, and folders that end up with no chapter are left out. | SHOULD BE (#306) | — | — |
| BOOK-002 | **The folder note** introduces a folder: `index.md`, else `README.md`, else the note named like the folder (`Writing/Writing.md`), else the one note of the folder whose front matter has `folder-note: true` (any case of the names). Its text is printed first in the folder's part, under the part's heading, and it is not a chapter of its own. A folder without one gets a part heading with the folder's name and no text. | SHOULD BE (#306) | — | — |
| BOOK-003 | **The reading order** of a folder's notes and subfolders: first the ones the folder note's `order:` property names (a list of note names without `.md`, or folder names; names that are not in the folder are ignored); without `order:`, the ones its wiki links point to, in the order of their first link (a link to a subfolder's folder note counts for the subfolder); then all others by name, compared naturally (`2 Lists` before `10 Tables`, case-insensitive), notes and folders mixed. A note is printed once, at its first place. | SHOULD BE (#306) | — | — |
| BOOK-004 | **Titles.** A chapter's title is its title line (VAULT-025: the file name without `.md` and a `YYMMDD_` date prefix), or its front matter `title:`; a part's title is the folder note's `title:`, else the folder's name. The book's title, `subtitle`, `author` and `date` come from the front matter of the top folder's note; the title falls back to the folder's name, for the vault root to the vault's name. | SHOULD BE (#306) | — | — |

## The PDF

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOOK-005 | **Layout.** The PDF has: a title page (title, subtitle, author, date, centred; when "Title page" is on), the table of contents (when "Table of contents" is on), then the top folder note's text, then the parts and chapters in reading order. Every chapter and part starts on a new page (the box "Each note on a new page", on by default; off: a gap of two lines). The paper, margins, font size, embeds, plain tables and page numbers of the note export dialog apply to the whole book. | SHOULD BE (#306) | — | — |
| BOOK-006 | **Headings and numbers.** A part's title is a heading of the level of its depth (the top folder's subfolders level 1), a chapter's title one level below its folder, and the headings inside a note go below the chapter (a `#` under a chapter of level 1 is level 2; nesting as in LINK-014). With numbering on (CFG-042) parts and chapters are numbered by their place (`2`, `2.1`, `2.1.3`) and the headings of a note continue the chapter's number (`2.1.1`); a note's own front matter cannot switch the book's numbering. A `#` heading at the start of a note that says the chapter's title is not printed again (PDF-008). | SHOULD BE (#306) | — | — |
| BOOK-007 | **Contents and bookmarks.** The contents list the parts, the chapters and the first level of headings inside each chapter, indented by level, with their numbers, a dotted leader and the page; each line links to its page. The bookmarks of PDF readers have every part, chapter and heading, nested by level. | SHOULD BE (#306) | — | — |
| BOOK-008 | **Running header and footer.** From the first chapter on, every page but a part's or chapter's first page has the chapter's title (with its number) in small grey text at the top right and a thin line under it; the page number (`n / N`) is at the bottom as in PDF-001 when page numbers are on. The title page has neither. | SHOULD BE (#306) | — | — |
| BOOK-009 | **Links inside the book.** A wiki link from one chapter to a note that is in the book (`[[Tables]]`, `[[Tables#Grid]]`, with alias) is a link to that chapter's page (or the heading's page) in the PDF; a link to a note outside the book is printed as its text, without a link; web links stay web links (PDF-001). | SHOULD BE (#306) | — | — |

## Dialog and command line

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOOK-010 | **Export to PDF…** is in the menu of a folder in the file tree and in the menu of the vault (the tree's top), and opens the export dialog for the folder: the built-in engine only, the options of BOOK-005 and a collapsible **Reading order** (the tree of parts and chapters as numbered, with the notes left out by `export: false` greyed), so the order can be checked before exporting. The default path is `<vault>/exports/<folder name>.pdf`, for the vault `<vault>/exports/<vault name>.pdf`. The export runs as a background job (JOB-001) with progress and Cancel. | SHOULD BE (#306) | — | — |
| BOOK-011 | `shale --export-pdf <folder> <file.pdf>` writes the book of a folder (BOOK-001…BOOK-009, default options: A4, numbering on, title page, contents, each note on a new page) and exits without opening a window: 0 on success, 1 with the reason on stderr. The folder is opened as a vault. `scripts/userguide-pdf.sh [out.pdf]` makes the user guide's PDF with it (default `target/userguide.pdf`). | SHOULD BE (#306) | — | — |
| BOOK-012 | **The user guide is a book.** `docs/userguide` follows the convention: "Start here" is its folder note (`folder-note: true`, the book's `title`, `subtitle`), its links give the chapter order, the example contact note is `export: false`. A test exports the guide as a book and checks that every note of the guide but the excluded one is in it once, in the order of "Start here", that no `![[` and no `[[` of a resolved link is printed as text, and that it has contents, bookmarks and running headers. | SHOULD BE (#306) | — | — |

# Spec: PDF export through LaTeX templates (TEX)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decisions: [ADR-0019](../decisions/0019-latex-export-managed-texlive.md).
Umbrella issue: #277. The built-in pure-Rust export (PDF-001…PDF-007, `src/pdf/`) stays the
default; the LaTeX path is a second engine the person picks per export (TEX-016). Tests are named
`file.rs::test_fn` (under `src/`).

## Engine

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TEX-001 | Shale looks for a LuaLaTeX to use: a `lualatex` found through `PATH` is accepted when its LaTeX kernel is 2022-06-01 or newer (the date tuda-ci needs) and `biber` is found next to it; otherwise the managed TeX Live (TEX-002) is used. | SHOULD BE (#279) | — | — |
| TEX-002 | The managed TeX Live is installed only after the person agrees: the dialog shows the download and disk sizes first, installs with the profile `assets/tex/texlive.profile` and the packages of `assets/tex/packages.txt` into `<data dir>/shale/texlive` (`dirs::data_dir()`), shows progress, and can be cancelled (a cancelled install leaves nothing half-installed). Nothing is downloaded without that consent. | SHOULD BE (#279) | — | — |
| TEX-003 | The managed TeX Live can be updated and removed from the settings; removing deletes its folder and nothing else. | SHOULD BE (#279) | — | — |
| TEX-004 | A LaTeX export runs LuaLaTeX and biber (as often as needed, like `latexmk`) in a fresh temporary folder, as a background job (JOB-001) with a timeout, without a shell; the window never blocks. The first run after an install builds the font cache and says so. On failure the dialog shows the readable errors from the `.log` (file, line, message), not the raw log; the temporary folder is removed afterwards. | SHOULD BE (#279) | — | — |

## Template sets

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TEX-005 | Template sets live in the vault, in `templates/tex/<set>/`: the `.cls`/`.sty`/`.def` files of the set and one `template.toml` per document kind (name, class, options, the mapping of TEX-010…TEX-014), so a vault carries its templates and TeX Live supplies only the engine and generic packages. | SHOULD BE (#280) | — | — |
| TEX-006 | A set is imported from CTAN, from a GitHub repository or release, or from a local zip; the `.dtx`/`.ins` files are run through docstrip when the set ships no ready `.cls`. A re-import updates the set's files and keeps the person's settings (`template.toml` values they changed, the logo). | SHOULD BE (#280) | — | — |
| TEX-007 | A template with a logo that cannot be redistributed (the TU Darmstadt `tuda_logo.pdf`) has a logo slot in its set: the person supplies the file; until then the export says so, and the compile does not fail on the missing file. | SHOULD BE (#280) | — | — |

## Markdown to LaTeX

H1 is the document title: it is never numbered and never listed in the table of contents
(owner decision, #283). The writer uses `ExportOptions.toc: bool`, `ExportOptions.numbered_headings` (the vault default) and, per note, `markdown::headings_numbered(text, opts.numbered_headings)` / `markdown::heading_numbers(text)` (PDF-008, CFG-042).

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TEX-008 | A Markdown writer turns a note into LaTeX: the TeX special characters are escaped (`\ { } $ & # ^ _ ~ %`), Unicode stays Unicode (LuaLaTeX), emphasis, strike-through, inline and fenced code, block quotes, ordered, unordered and task lists, tables, links (`\href`), footnotes and rules are written. Text from the note can never end the document or run a command. | SHOULD BE (#281) | — | — |
| TEX-009 | Headings: H1 is the title (`\title`), H2 is `\section`, H3 `\subsection`, H4 `\subsubsection` (article-like kinds, TEX-010); nesting decides the level, so an H4 directly under an H2 is the next level down, no level is skipped. The numbers follow the note's setting (`headings_numbered`, starred commands when off); the table of contents lists H2–H4 only, and only when `ExportOptions.toc` is on. | SHOULD BE (#281) | — | — |
| TEX-010 | Article-like kinds (pub, report, exercise) write the body as TEX-009 says into the class of the set. | SHOULD BE (#281) | — | — |
| TEX-011 | Book-like kinds (thesis, phd) map H2 to `\chapter`, then H3 and H4 one level down each (`\section`, `\subsection`); H1 is the title. | SHOULD BE (#281) | — | — |
| TEX-012 | The beamer kind maps every H2 to a `frame` (its title the frame title) with the content below it up to the next H2; H1 is the title slide. | SHOULD BE (#281) | — | — |
| TEX-013 | The letter kind takes sender, recipient, subject and date from the note's properties and the body from the text. | SHOULD BE (#281) | — | — |
| TEX-014 | Poster, sciposter, leaflet and announcement kinds are written to their classes with the mapping of their `template.toml`. | SHOULD BE (#281) | — | — |
| TEX-015 | Pictures are written with `\includegraphics` (a file copied into the temporary folder, sized as in the note, PDF-005), drawings as vector PDF made by the built-in export, and a note's colours (PDF-006) as `xcolor` colours. | SHOULD BE (#281) | — | — |

## Dialog and setup

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TEX-016 | The note export dialog offers the engine (built-in or LaTeX) and, for LaTeX, the template set, the kind and the logo; the built-in engine stays selected until the person chooses otherwise. | SHOULD BE (#282) | — | — |
| TEX-017 | A guided setup runs on the first LaTeX export: find or install the engine (TEX-001, TEX-002), import a template set (TEX-006), ask for the logo (TEX-007). The user guide describes it. | SHOULD BE (#282) | — | — |

## Test stack

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TEX-020 | A podman test stack, `tests/tex-env/`, builds an image with TeX Live (LuaLaTeX, biber) from a frozen repository (TL2025 final) and the packages of `assets/tex/packages.txt`, fetches tuda-ci 4.06 at a pinned commit checked by sha256, generates its classes with docstrip, and compiles with `tex-env.sh compile`. | IS NOW | `tests/tex-env/tex-env.sh`, `tests/tex-env/Containerfile`, `assets/tex/` | `pdf/tex/env_tests.rs::every_tuda_ci_demo_compiles_to_a_pdf_with_pages_tex_env` |
| TEX-021 | Every one of the 25 tuda-ci demos (thesis, phd, report, pub, exercise, letter, beamer, poster, sciposter, leaflet, announcement in German and English, the 2008 beamer theme and the two example files) compiles with LuaLaTeX and biber, with graphicx' `demo` option instead of the logo, to a PDF with at least one page. | IS NOW | `pdf/tex/env_tests.rs` | `pdf/tex/env_tests.rs::every_tuda_ci_demo_compiles_to_a_pdf_with_pages_tex_env` |
| TEX-022 | The package list `assets/tex/packages.txt` has one TeX Live package per line (comments after `#`), no duplicates, and is the one list used by both the managed install and the test image. | IS NOW | `assets/tex/packages.txt`, `tests/tex-env/tex-env.sh` | `pdf/tex/env_tests.rs::the_package_list_is_one_name_per_line_without_duplicates` |

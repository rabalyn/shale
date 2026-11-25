# Hochschule Hannover templates in Shale

How to export notes from Shale with the LaTeX templates of the Hochschule Hannover
([hsh-elc/hsh-latex-templates](https://github.com/hsh-elc/hsh-latex-templates), commit `650a580`,
2025-03-03) instead of the TU Darmstadt ones, what of the TU Darmstadt approach carries over, and
what the Typst thesis [MrToWy/Bachelorarbeit](https://github.com/MrToWy/Bachelorarbeit) means for it.

Written 2026-11-25. Everything below "Set it up" was compiled with the TeX Live of Shale's test image
(`localhost/shale-tex-env`), from a document of the shape Shale's LaTeX writer produces.

## In short

- **The HsH templates work in Shale today, by hand**: one folder per template under
  `templates/tex/`, one small wrapper package, one kind file. Thesis (title page with faculty logo,
  declaration, Roman front matter, contents, chapters, citations with biber, references) and slides
  (HsH title slide and faculty colours) both compile.
- **The one-click way of the TU Darmstadt does not carry over.** *Import from GitHub…* refuses the
  repository ("no class or package files (.cls, .sty) were found"), and even when it does not, it
  drops the logos (`.pdf` files are not copied).
- **The MrToWy thesis is not LaTeX but [Typst](https://typst.app)** (`main.typ`, "Creator: Typst 0.12.0"
  in its `main.pdf`), built on the Typst template
  [`modern-hsh-thesis`](https://github.com/MrToWy/hsh-thesis). Shale cannot use it, and its look is
  not the look of the official HsH LaTeX template: see "The MrToWy thesis" below.

## How the HsH templates differ from tuda-ci

| | TU Darmstadt (tuda-ci) | Hochschule Hannover |
|---|---|---|
| What it is | document classes (`tudapub.cls`, `tudabeamer.cls`, …) built from `.dtx` | three **example documents** to copy and edit: `Abschlussarbeiten/`, `Berichte/`, `Präsentationen/`, each a `main.tex` with `\input` files |
| Class | its own | KOMA-Script `scrbook` / `scrartcl`, `beamer` |
| Title page | `\maketitle` of the class | a file `0a-Titelblatt.tex` that `main.tex` puts into the body |
| Metadata | `\title`, `\author`, `\reviewer`, … (one argument) | `\Titel`, `\Fakultaet`, `\Studiengang`, `\DatumAbgabe`, … and `\VerfasserIn[label]{name}{matr.-no.}` (three arguments) |
| Logo | one file the university does not let anyone redistribute; the person supplies it | **in the repository**, one per faculty (`hsh/HsH-Logo-F1.pdf` … `F5`, `grau`, `gelb`), chosen by a number: `\LogoHsH{4}` (but excluded from the CC BY-SA licence, so Shale should fetch rather than ship them) |
| Colour | accent code `0a`–`11d` | faculty number `0`–`6` (`HsH-Farben.tex`) |
| Engine | LuaLaTeX | written for pdfLaTeX (`inputenc`, `fontenc` T1, `lmodern`); compiles under LuaLaTeX with one harmless warning |
| Extras | — | glossaries (needs `makeglossaries`), `listings`, `circuitikz`, `siunitx`, upright subscripts (an active `_` in math) |

## Set it up: the thesis

### 1. The folder

Make the folder `templates/tex/hsh-thesis/` in your vault and copy into its `tex/abschlussarbeiten/`
from the repository's `Abschlussarbeiten/` folder: `0a-Titelblatt.tex`, `tex/` and `hsh/` (the rest —
`main.tex`, the chapter files, `gls/`, `bib/` — is what your note replaces).

```
templates/tex/hsh-thesis/
  set.toml
  settings.toml
  kinds/thesis.toml
  tex/hsh-thesis.sty
  tex/abschlussarbeiten/0a-Titelblatt.tex
  tex/abschlussarbeiten/tex/prae-com.tex, prae-dev.tex, w-sep.tex, test-text.tex
  tex/abschlussarbeiten/hsh/HsH-Farben.tex, HsH-Logo-*.pdf
```

**One set per HsH template.** `Berichte/` and `Abschlussarbeiten/` both have a `tex/prae-com.tex` and a
`0a-Titelblatt.tex`; Shale searches a set's `tex/` with all its sub-folders, so two in one set would
be found at random. A report gets its own set, `templates/tex/hsh-report/`, built the same way.

### 2. `set.toml`

```toml
name = "Hochschule Hannover: Abschlussarbeit"
version = "650a580"
imported_at = "2026-11-25"

[source]
kind = "local"
path = "https://github.com/hsh-elc/hsh-latex-templates (by hand)"
```

No `logo_file`: the logos are in the set, so there is no logo slot, no placeholder and no
"logo missing" warning. **Do not press *Update set*** for this set: it would import again from that
path and fail.

### 3. `settings.toml`

```toml
accent = "4"          # the faculty: 0 grey, 1–5 Fakultät I–V, 6 yellow (central units)
language = "de"
department = "Fakultät IV -- Wirtschaft und Informatik"
author = "Jane Doe"
```

`accent` is reused for the faculty number (the kind puts it into `\LogoHsH{…}`); Shale's accent
swatches are those of the TU Darmstadt, so edit the file instead of using them. Leave
`citation_style` empty: the template fixes `ieee` itself (see "Limits").

### 4. `tex/hsh-thesis.sty`: the wrapper

Shale writes every field as `\command{value}` and starts the document with `\maketitle`. The
wrapper loads the template's preamble and title page **unchanged** and adds only that:

```latex
% hsh-thesis.sty: the thesis template of the Hochschule Hannover (hsh-elc/hsh-latex-templates,
% folder Abschlussarbeiten) as a package, so that Shale can use it as a document kind.
\NeedsTeXFormat{LaTeX2e}
\ProvidesPackage{hsh-thesis}[2026/11/25 HsH thesis template for Shale]
\input{tex/prae-com}
% every value the title page prints, so a field left empty does not stop the export
\Fakultaet{}\Fachgebiet{}\ArtDerArbeit{Abschlussarbeit}\Studiengang{}\Titel{}\Untertitel{}
\VerfasserIn{}{}\DatumBeginn{}\DatumAbgabe{\today}\DatumVersion{\today}
\BetreuerInA[Erstprüfer:in]{}\BetreuerInB[Zweitprüfer:in]{}\BetreuerInC{}
% Shale writes every field as \command{value}: one argument each
\newcommand{\HsHAutor}[1]{\def\VarVerfasserIn{#1}}
\newcommand{\HsHMatrikelnummer}[1]{\def\VarVerfasserInMatNr{#1}}
\newcommand{\HsHErstpruefer}[1]{\def\VarBetreuerInA{#1}}
\newcommand{\HsHZweitpruefer}[1]{\def\VarBetreuerInB{#1}}
\newcommand{\HsHBetreuer}[1]{\def\VarBetreuerInC{#1}\def\VarBezBetreuerInC{Betreuer:in}}
% Shale's document starts with \maketitle: the HsH title page and the declaration
\renewcommand{\maketitle}{%
  \pagenumbering{Roman}%
  \input{0a-Titelblatt}\cleardoublepage
  \thispagestyle{plain.scrheadings}%
  \section*{Eigenhändigkeitserklärung}
  Ich versichere, dass ich meine am \VarDatumAbgabe{} vorgelegte \VarArtDerArbeit{} selbstständig verfasst und alle benutzten Hilfsmittel angegeben habe.\\[48pt]
  Hannover, den \VarDatumAbgabe\\[40pt]
  \begin{tabular}{@{}p{5cm}l}\hline\VarVerfasserIn, \VarVerfasserInMatNr\end{tabular}
  \cleardoublepage
  \pagestyle{scrheadings}%
}
% the text starts with arabic page 1 at its first chapter (Shale writes no \mainmatter); the
% chapters of the contents and the lists before it keep the Roman numbers of the front matter
\newif\ifHsHbody \newif\ifHsHlist
\newcommand{\HsHStartBody}{\ifHsHbody\else\ifHsHlist\else\global\HsHbodytrue\pagenumbering{arabic}\fi\fi}
\AddToHook{cmd/@chapter/before}{\HsHStartBody}
\AddToHook{cmd/@schapter/before}{\HsHStartBody}
\AddToHook{cmd/tableofcontents/before}{\HsHlisttrue}\AddToHook{cmd/tableofcontents/after}{\HsHlistfalse}
\AddToHook{cmd/listoffigures/before}{\HsHlisttrue}\AddToHook{cmd/listoffigures/after}{\HsHlistfalse}
\AddToHook{cmd/listoftables/before}{\HsHlisttrue}\AddToHook{cmd/listoftables/after}{\HsHlistfalse}
```

### 5. `kinds/thesis.toml`

The class options are those of the template's `main.tex`.

```toml
# Hochschule Hannover, Bachelor- oder Masterarbeit (scrbook, hsh-thesis.sty). # is a \chapter.
name = "Abschlussarbeit (HsH)"
description = "Bachelor- und Masterarbeit der Hochschule Hannover"
mapping = "book"
class = "scrbook"
options = ["a4paper", "12pt", "parskip=half", "numbers=noendperiod", "DIV=15", "toc=chapterentrywithdots"]
preamble = ["\\usepackage{hsh-thesis}", "\\LogoHsH{{accent}}"]

[[fields]]
key = "title"
label = "Titel"
command = "Titel"
required = true

[[fields]]
key = "subtitle"
label = "Untertitel"
command = "Untertitel"

[[fields]]
key = "author"
label = "Verfasser:in"
command = "HsHAutor"
required = true

[[fields]]
key = "matrikelnummer"
label = "Matrikelnummer"
command = "HsHMatrikelnummer"

[[fields]]
key = "type"
label = "Art der Arbeit"
command = "ArtDerArbeit"
default = "Bachelorarbeit"

[[fields]]
key = "programme"
label = "Studiengang"
command = "Studiengang"

[[fields]]
key = "department"
label = "Fakultät"
command = "Fakultaet"

[[fields]]
key = "group"
label = "Fachgebiet / Abteilung"
command = "Fachgebiet"

[[fields]]
key = "startdate"
label = "Bearbeitungsbeginn"
command = "DatumBeginn"

[[fields]]
key = "submissiondate"
label = "Abgabe"
command = "DatumAbgabe"
default = "\\today"

[[fields]]
key = "reviewer"
label = "Erstprüfer:in"
command = "HsHErstpruefer"

[[fields]]
key = "second_reviewer"
label = "Zweitprüfer:in"
command = "HsHZweitpruefer"

[[fields]]
key = "supervisor"
label = "Betreuer:in (Firma)"
command = "HsHBetreuer"
```

### 6. The packages

`prae-com.tex` loads packages that Shale's TeX Live does not install: `setspace`, `glossaries`
(with `mfirstuc`, `xfor`, `datatool`, `tracklang`), `listings`, `circuitikz`; the slides also need
`multirow`, `appendixnumberbeamer` and `colortbl`. First **Settings → LaTeX → Update** (an older
managed TeX Live may also lack `caption`, `scrhack` and `biblatex-ieee`), then, in a terminal:

```bash
~/.local/share/shale/texlive/bin/x86_64-linux/tlmgr install setspace glossaries mfirstuc xfor datatool tracklang listings circuitikz multirow appendixnumberbeamer colortbl
```

(On macOS and Windows the `bin/` folder has the platform's name.) A TeX Live of your own that has
them works too (`tex.engine` = System). Or copy `prae-com.tex` and delete what you do not use.

### 7. A note

```markdown
---
latex-template: hsh-thesis/thesis
title: Reglerentwurf für ein Elektrokleinfahrzeug
subtitle: im Rahmen des Projekts XY
author: Jane Doe
matrikelnummer: 1234567
type: Masterarbeit
programme: Elektrische Energiesysteme
group: Abteilung Informatik
startdate: 1. September 2026
submissiondate: 28. Februar 2027
reviewer: Prof. Dr. Erika Musterfrau
bibliography: refs.bib
---

# Einleitung
Text mit Zitat [@knuth] …
```

**⋯ → Export to PDF… → LaTeX template** picks the set and kind from `latex-template`. What came out
of the test: page I the HsH title page with the logo of Fakultät IV and all fields, page III the
declaration, page V the contents, page VII the list of figures, then chapter 1 on arabic page 1,
the citation `[1]` and the list of references ("Literatur") in the contents, `\cref` as "Kapitel 1",
`U_max` with an upright index.

## The slides

Same steps, set `templates/tex/hsh-slides/`, with the repository's `Präsentationen/` folder copied to
`tex/praesentationen/` (without `main.tex`, `tex/blindframes.tex` and `bib/`):

```latex
% tex/hsh-slides.sty
\NeedsTeXFormat{LaTeX2e}
\ProvidesPackage{hsh-slides}[2026/11/25 HsH slides template for Shale]
\input{tex/packages-com}
\input{tex/new-commands}
\input{hsh/HsH-Farben}
\newcommand{\HsHSlidesSetup}[1]{\HsHFak{#1}\input{tex/beamer-options}}
```

```toml
# kinds/beamer.toml
name = "Folien (HsH)"
description = "Präsentation der Hochschule Hannover (beamer, 16:9)"
mapping = "beamer"
class = "beamer"
options = ["ngerman", "hyperref={bookmarks}", "xcolor={table,dvipsnames}", "aspectratio=169", "usepdftitle=false"]
preamble = ["\\usepackage{hsh-slides}", "\\HsHSlidesSetup{{accent}}"]

[[fields]]
key = "title"
label = "Titel"
command = "title"
required = true

[[fields]]
key = "subtitle"
label = "Untertitel"
command = "subtitle"

[[fields]]
key = "author"
label = "Autor:in"
command = "author"
required = true

[[fields]]
key = "date"
label = "Datum"
command = "date"
```

Tested: the HsH title slide (wordmark, big faculty-coloured H) and frames in the colour of the
faculty. Not tested: the `table` option of `xcolor` (the test image has no `colortbl`).

## Limits

- **Front and back matter.** Shale makes title, contents, lists, chapters and references. Foreword,
  abstract, glossary, list of symbols and appendix of the template have no place in Shale's model:
  write them as chapters of the note (an unnumbered heading for the foreword). The glossary would
  need `makeglossaries`, which Shale does not run.
- **Citation style.** `prae-com.tex` loads `biblatex` with `style=ieee`, and Shale then does not load
  it again, so the dialog's *Citation style* has no effect. To choose there, delete the `biblatex`
  line from `prae-com.tex`.
- **Underscore.** The template makes `_` active in math (upright subscripts) and `_` an ordinary
  character in text. Shale's output survived that in the test (`U_{max}`, `file\_name`), but a
  formula that relies on italic subscripts will change.
- **Settings → LaTeX → Test page** looks for a `pub` or an article kind; a thesis-only set has none,
  so test by exporting a note.
- **The guided setup** knows only the TU Darmstadt: its template, logo and colour steps do not apply.

## What of the TU Darmstadt approach carries over

| Part | Carries over? |
|---|---|
| A set is a folder of the vault, synced and backed up with it | yes |
| Kinds as TOML: class, options, preamble, fields → commands, `{accent}`/`{language}` filled in | yes, and it is flexible enough: the wrapper above is all the adaptation needed |
| Heading mapping (`book`, `beamer`), thesis syntax (biblatex/biber, `cleveref`, captions, guarded by `\IfPackageLoadedTF`) | yes |
| `TEXINPUTS` over the set's `tex//` and `logo//` (finds `hsh/HsH-Logo-F4` in a sub-folder) | yes |
| One-click import (built-in preset, pinned commit, checksum) | **no**: the preset is hard-wired to tuda-ci (`import.rs` `TUDA`), and a generic import needs a `.cls`/`.sty` and copies only TeX files, not the logos |
| Logo slot (supplied by the person, placeholder, "not CI-conformant") | **no**: the HsH logos come with the templates and are chosen by faculty number |
| Accent colour swatches, guided setup steps 2–4 | **no**: TU Darmstadt only |
| The managed TeX Live's package list | **partly**: it is "whatever makes the 25 tuda-ci demos compile"; HsH needs about ten more |
| Field = one-argument command | **mostly**: multi-argument macros like `\VerfasserIn` need a one-argument setter in a wrapper |
| `\maketitle` starts the document | **no** for "example document" templates: the wrapper has to make `\maketitle` the title page |

## The MrToWy thesis

[MrToWy/Bachelorarbeit](https://github.com/MrToWy/Bachelorarbeit) (release 1.6.2, `main.pdf`, 128
pages) is written in **Typst**, and its template became the Typst package
[`modern-hsh-thesis`](https://github.com/MrToWy/hsh-thesis) (v1.1.1, `typst init
@preview/modern-hsh-thesis`, for Fakultät IV). Shale's template engine is LaTeX only, so:

- **Shale cannot render it, and no Shale export can match its `main.pdf`.** That PDF is set in
  Libertinus Serif with Cascadia Code, its title page has the HsH wordmark top left, the title, author
  and date in the left column and a large orange H below; it has coloured requirement boxes and its
  own heading style.
- **Its look is not the HsH LaTeX template's.** The LaTeX template centres "Hochschule Hannover",
  faculty, kind of work and title under a smaller H, with a table of author, dates and examiners at
  the bottom, set in Latin Modern. The two are different designs of the same university; neither is
  the other.
- Getting the MrToWy look in Shale would mean either a LaTeX port of the Typst template (a new design
  to maintain, approximate) or Typst as a second template engine (below).

## Toward "one-click templates"

What the HsH case shows about a catalog that people can use without knowing LaTeX:

1. **A curated catalog, not a generic import.** Templates in the wild are example documents, not
   classes. What makes one usable is the adapter: the wrapper, the kind files, the field names, the
   packages it needs, a faculty or colour choice. That adapter is the catalog entry; the template
   files are fetched from upstream at a pinned commit with a checksum (as for tuda-ci), which also
   keeps logos like the HsH ones out of Shale's own distribution.
2. **A catalog entry is data, not code**: generalise the hard-wired `TUDA` preset into a manifest
   (upstream repository and commit, checksum, which folders to copy — including images —, the
   wrapper and kinds to add, extra TeX packages to `tlmgr install`, the settings choices and their
   labels, e.g. "Fakultät" instead of "accent"). HsH thesis, report and slides would be the first
   entries after tuda-ci.
3. **Typst is worth a serious look as a second engine.** Typst is a Rust library (Shale could embed
   it: no 330 MB TeX Live, compiles in milliseconds), and **Typst Universe is already a catalog** of
   versioned thesis templates installed by name — including `modern-hsh-thesis`. A Markdown → Typst
   writer would mirror the LaTeX writer. This is a decision for an ADR, not a detail.

Issues: the catalog #703, the import gaps #704, Typst #705.

# Catalogue of real-world LaTeX thesis templates (for Shale template sets)

Research date: 2026-11-25. Method: each GitHub repo was shallow-cloned (`git clone --depth 1`) and its
`.cls/.sty/.dtx` and main `.tex` files were grepped and read; stars, licence and last-commit date come
from one GitHub GraphQL query (`stargazerCount`, `licenseInfo.spdxId`, date of the default branch's head
commit). CTAN-only packages (classicthesis, mitthesis, toptesi) were read from `mirrors.ctan.org`;
CTAN versions/licences come from `https://ctan.org/json/2.0/pkg/<id>`. Nothing was compiled: every
"breaks under LuaLaTeX" statement is a reading of the source, not a test run, and is marked as such.
"none" in the licence column means GitHub reports no licence file (all rights reserved by default).

Legend for the table

- **Type**: `CLS` = real class/package, `\documentclass{x}` works on its own; `DTX` = class built from
  `.dtx/.ins` (usually also on CTAN); `EX` = example document (main.tex + `\input` files, stock base
  class); `EX+STY` = example document plus its own small `.sty` that holds the title page/metadata
  commands (the pattern Shale's HsH wrapper reproduces); `TYP` = Typst.
- **Title**: `MT` = class `\maketitle` (possibly redefined by the class/sty); `MT-redef-in-body` =
  `\maketitle` redefined in a file the user `\input`s; `FILE` = separate title-page file / `titlepage`
  environment `\input` in the body; `CMD` = own command instead of `\maketitle` (e.g. `\dhbwtitlepage`).
- **Meta**: `std` = `\title\author\date`; `1arg` = own one-argument setters (`\supervisor{…}`);
  `multi` = multi-argument setters; `kv` = key-value (`\setup{key=val}`, pgfkeys, l3keys, package
  options); `vars` = `\newcommand{\myTitle}{…}` variables read by a title file; `typed` = values typed
  directly into the title-page file.
- **Engine**: `pdf` = pdfLaTeX-oriented (inputenc/T1 fontenc, pdfTeX primitives); `lua` = needs or
  prefers LuaLaTeX/XeLaTeX (fontspec); `any` = branches on `iftex` or nothing engine-specific.
- **Bib**: `biber` = biblatex+biber; `bibtex` = natbib/plain BibTeX; `pandoc` = templated.

## 1. Summary table

| # | Template | Institution | Type | Base class | Title | Meta | Logos | Engine | Bib | Stars | Last update | Licence |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | TUDa-CI `tudapub` | TU Darmstadt | DTX (CTAN) | KOMA (scrreprt/scrbook/scrartcl via `class=`) | MT | std + 1arg (+ class kv opts) | user supplies (`tuda_logo.pdf`, licence-excluded, installer script) | any (fontspec under Lua) | biber | 254 | 2026-06-09 (CTAN 4.08, 2026-08-19) | LPPL-1.3c |
| 2 | TUD-Script `tudscrreprt` | TU Dresden | DTX (CTAN) | KOMA | MT | std + 1arg (some nested in `\author`) | DDC logos shipped; TUD logo via package (unverified how) | any (Open Sans; fontspec branch) | any (doc uses biber) | 118 | 2022-08-09 (CTAN 2.06o) | LPPL-1.3c |
| 3 | netd-tud teaching template | TU Dresden (chair NetD) | EX+STY | scrbook | MT (sty redefines) | std + 1arg | shipped (new TUD logo) | any (iftex) | biber | 5 | 2026-04-17 | none |
| 4 | tum-thesis-latex | TUM (unofficial) | EX | scrbook | FILE (`pages/cover`, `pages/title`) | vars (`\getTitle` …) | user supplies (empty `logos/`, crop script) | pdf | biber | 465 | 2022-12-20 | none |
| 5 | TTT | TUM (unofficial) | EX | scrbook | FILE | vars (`\authorname` …) | shipped | pdf (`[pdftex]` class option) | bibtex | 28 | 2017-11-26 | WTFPL |
| 6 | TUM CSE template | TUM CSE | EX | scrbook | FILE | vars (`\def\author` …) | shipped | pdf (minted) | bibtex | 53 | 2020-05-08 | none |
| 7 | scientific-thesis-template | Uni Stuttgart / Uni Hamburg / TH Ulm | EX+STY (sty on CTAN) | scrbook | CMD (`\Titelblatt`) | kv (package options of `scientific-thesis-cover`) | shipped (UHH, THU) | any (LuaLaTeX default in latexmkrc) | biber | 396 | 2026-11-20 | 0BSD |
| 8 | uni-stuttgart-dissertation-template | Uni Stuttgart | EX+STY | scrbook | MT (`ustutt-titlepage.sty` redefines) | std + 1arg | shipped | any (LuaLaTeX branch) | biber | 58 | 2026-11-20 | MIT |
| 9 | tudothesis | TU Dortmund (physics) | CLS | scrbook | MT | std + 1arg | shipped | lua (fontspec + unicode-math in main.tex) | biber | 50 | 2023-07-19 | MIT |
| 10 | tubslatex | TU Braunschweig | DTX | KOMA (via `\BaseClass`) | MT (redef, kv option) | std + 1arg + kv | `cd-base/logo` (shipped, licence unclear) | pdf (inputenc everywhere) | n/a | 8 | 2015-04-20 | none |
| 11 | TUB_PhDThesisTemplate | TU Berlin (unofficial) | CLS | book | MT (redef) | std + 1arg | user/crest | any (ifluatex, PDF/A) | biber or bibtex (option) | 39 | 2017-11-10 | MIT |
| 12 | KIT OS group template | KIT | EX+STY | (scrbook-like, see §) | MT (`kitthesiscover.sty` redefines) | std + 1arg (`\advisor` 2-arg) | shipped | **pdf-only** (`\pdfximage`) | bibtex | 12 | 2019-07-30 | none |
| 13 | kit-thesis-template | KIT | TYP | – | – | – | – | Typst | – | 4 | 2025-05-17 | none |
| 14 | lmu-thesis-latex | LMU München | EX+STY | article | CMD (`\makeabstract`, no `\maketitle`) | std | shipped | pdf (latin1 inputenc) | bibtex | 16 | 2019-05-22 | MIT |
| 15 | hsh-latex-templates | Hochschule Hannover | EX | scrbook | FILE (`0a-Titelblatt`) | multi (`\VerfasserIn[label]{name}{matnr}`) | shipped | pdf | biber | 0 | 2025-03-03 | CC-BY-SA-4.0 |
| 16 | HagenbergThesis `hgbthesis` | FH Oberösterreich (AT) | CLS (CTAN) | book | MT (redef, themes) | std + 1arg + multi (`\dateofsubmission{Y}{M}{D}`) + kv class opts | shipped (themes) | any (T1 + iftex; Lua documented) | biber | 235 | 2026-10-22 | CC-BY-4.0 / CC-BY-SA |
| 17 | vutinfth | TU Wien Informatics (AT) | DTX | memoir | CMD (`\addtitlepage{lang}`) | multi (`\setauthor{pre}{name}{post}{gender}`) | shipped | pdf | bibtex | 3 (mirror) | 2017-06-19 | none (mirror) |
| 18 | abschlussarbeit-tuwien-physik | TU Wien Physics (AT) | EX | scrreprt | FILE (`\titlePageTUWienPhysik`) | kv (`\setkeys` in title file) | shipped | pdf (pdfx, `\ifpdf` branch) | biber | 20 | 2021-05-19 | MIT |
| 19 | ethz-thesis | ETH Zürich (CH) | EX | scrreprt (classicthesis) | FILE | vars (`\myName` …) | none | pdf | biber | 117 | 2017-03-04 | none |
| 20 | ZHAW thesis (MastersDoctoralThesis) | ZHAW (CH) / LaTeXTemplates.com | CLS (MDT) + EX | book/report (`\baseclass`) | FILE (`Front/titlepage`) | 1arg (`\thesistitle`, `\supervisorA` …) | shipped | any (ifxetex branch) | biber | 46 | 2024-10-28 | cls LPPL-1.3c, repo none |
| 21 | uzh-ma-thesis | Uni Zürich (CH) | EX (+pandoc template) | report | FILE (`titlepage` env) | typed / pandoc vars | shipped | any | bibtex (natbib) | 12 | 2021-12-08 | MIT |
| 22 | HSLU BA template | Hochschule Luzern (CH) | EX | scrreprt | FILE | typed | shipped | pdf (pdfx) | biber | 32 | 2022-10-21 | none |
| 23 | iodhbwm | DHBW Mannheim | CLS (CTAN) | KOMA (configurable) | CMD (`\dhbwtitlepage`) or MT | kv (`\dhbwsetup{…}`, pgfkeys) | shipped | pdf (examples T1) | biber (class option) | 20 | 2023-01-02 (CTAN 1.2.2) | LPPL-1.3 |
| 24 | dhbw-stuttgart-template | DHBW Stuttgart | EX | scrreprt | FILE | vars (`\documentTitle` …) | shipped | pdf | biber | 27 | 2026-03-26 | none |
| 25 | dhbw-heidenheim-latex-template | DHBW Heidenheim | EX | scrreprt | FILE | vars | shipped | pdf | biber | 32 | 2019-10-15 | MIT |
| 26 | FH-Aachen Vorlage | FH Aachen | EX | scrreprt | FILE | typed | shipped | pdf | bibtex (natbib, unsrtdin) | 7 | 2016-07-04 | none |
| 27 | Hochschule-Muenchen-LaTeX-Template | HM München | EX | book | FILE | typed | shipped | pdf | biber | 25 | 2025-10-26 | unclear |
| 28 | hm-seclab thesis-template | HM München (SecLab) | EX+CLS (`zyx/mythesis.cls`) | scrbook | FILE (`zyx/preface.tex`) | vars (`\myTitle` …) | shipped (unverified) | pdf | biber | 26 | 2025-05-19 | Unlicense |
| 29 | htw-berlin-latex-thesis-template | HTW Berlin | EX | scrreprt | MT-redef-in-body (`resources/titlepage`) | std + 1arg | user supplies (zip) | lua (fontspec) | none set | 4 | 2024-07-08 | Unlicense |
| 30 | HSMZ-Thesis-Template | Hochschule Mainz | EX | scrreprt | FILE | vars (`\titel`, `\autor` …) | shipped (unverified) | pdf (minted) | biber | 19 | 2026-03-02 | none |
| 31 | hbrs-latex-vorlage-arbeit | H-BRS Bonn-Rhein-Sieg | EX+STY | scrreprt | FILE (`abschnitte/Deckblatt`) | vars (`\ThesisTitle` …) | shipped | lua (fontspec) | biber | 13 | 2026-06-22 | AGPL-3.0 |
| 32 | latex-vorlage-masterarbeit | TH Nürnberg (Ohm) | EX | scrreprt | FILE (`Deckblatt`) | vars (`\titel`, `\autor`) | shipped | pdf (latin1!) | bibtex (natdin) | 27 | 2015-05-24 | unclear |
| 33 | latex-vorlage-diplom-bachelor-masterarbeiten | generic FH | EX | scrartcl | FILE (`deckblatt`) | vars + typed | shipped (placeholders) | pdf | bibtex (alphadin) | 77 | 2022-08-28 | none |
| 34 | LaTeX-Master-Vorlage | generic DE | EX | scrbook | MT-redef-in-body (`titles/graduation`) | std + 1arg | none | pdf | biber | 48 | 2021-03-10 | unclear |
| 35 | Clean Thesis `cleanthesis` | generic (DE author) | EX + STY (CTAN) | scrreprt | FILE (`content/titlepages`) | vars (`\thesisTitle` …) | shipped (placeholder) | pdf | biber (sty option) | 922 | 2021-02-08 (CTAN 0.4.0) | none (CTAN LPPL-1.3) |
| 36 | classicthesis | generic (André Miede) | EX + STY (CTAN) | scrreprt/scrbook | FILE (`FrontBackmatter/Titlepage`) | vars (`\myTitle`, `\myName` …) | none | any (fontspec branch) | biber (natbib compat) | 56 (stale fork; real repo on Bitbucket) | CTAN 4.8, 2025-02-02 | GPL-2.0+ |
| 37 | kaobook | generic | CLS | scrbook | MT (KOMA) | std (KOMA `\titlehead`, `\publishers`) | none | any (iftex) | biber | 1122 | 2025-12-23 | LPPL-1.3c |
| 38 | latex-mimosis | generic (Heidelberg/ETH title examples) | CLS + EX | scrbook | FILE (`Sources/Title`) reading `\@title` | std | none | any (iftex) | biber | 1244 | 2026-05-18 | MIT |
| 39 | novathesis | NOVA Lisbon + ~30 schools (PT, also HU Berlin) | CLS | memoir | CMD (`\ntprint{frontcover}`) | multi + kv (`\nttitle(main,en){…}`, `\ntsetup{…}`) | shipped (per school) | lua (default), any | biber | 1014 | 2026-10-30 | LPPL-1.3c |
| 40 | Dissertate | Harvard + 15 US schools | CLS + per-school sty | book | MT (redef per school) | std + 1arg | shipped (some) | any | bibtex | 757 | 2026-11-25 | AGPL-3.0 |
| 41 | PhDThesisPSnPDF | Cambridge (CUED) | CLS | book | MT (redef) | std + 1arg (`\crest{…}` = logo) | user supplies crest | any (xe/lua branch) | bibtex or biblatex (option) | 905 | 2026-04-28 | MIT |
| 42 | ucl_thesis | UCL | CLS | book | MT (redef) + `\makedeclaration` | std + 1arg | none | pdf (`hyperref[pdftex]`) | bibtex | 195 | 2021-06-07 | unclear |
| 43 | mitthesis | MIT (official, Lienhard) | CLS (CTAN) | report | MT | std + multi (`\Author{name}{dept}`, `\Supervisor{…}{…}[…]`) | none | any (fontsets, unicode-math branch, tagged PDF) | biber | – (CTAN 1.25, 2026-11-23) | 2026-11-23 | MIT |
| 44 | suthesis-2e | Stanford | STY on `report` | report | MT (= `\titlep`) + `\beforepreface/\afterpreface` | std + 1arg | none | any | bibtex | 61 | 2026-11-25 | MIT |
| 45 | EPFL_thesis_template | EPFL (unofficial) | EX | book | FILE | typed | shipped | pdf | bibtex (natbib) | 136 | 2026-07-06 | MIT |
| 46 | ut-thesis | Univ. of Toronto | DTX (CTAN) | book | MT (redef) | std + 1arg | none | any | none set | 87 | 2024-07-26 (CTAN 3.1.8) | LPPL-1.3c |
| 47 | yathesis | French universities | DTX (CTAN) | book | MT (redef, `\maketitle[opts]`) | multi + kv (`\author{first}{last}`, `\yadsetup`, pgfkeys) | user supplies (`logo=` keys) | any (iftex) | biber (required for some options) | 36 | 2026-05-05 (CTAN 1.0.14) | LPPL-1.3c |
| 48 | thuthesis | Tsinghua (CN) | DTX (CTAN) | ctexbook | MT (redef) | kv (`\thusetup{title=…}`) | shipped | lua/xe (ctex, fontspec) | bibtex (natbib) or biblatex | 5499 | 2026-06-17 (CTAN 7.7.1) | LPPL-1.3c |
| 49 | adsphd | KU Leuven (BE) | CLS | report | MT (redef) | std + multi (`\author{first}{last}`, `\supervisor{name}{affil}`) + kv class opts | shipped | lua/xe (fontspec; Lua default) | bibtex or biblatex | 115 | 2026-09-02 | LPPL-1.2+ |
| 50 | thesis-template-Oxford | Oxford MSD (unofficial) | EX | report | FILE | typed | shipped (crest) | pdf | bibtex (natbib) | 10 | 2020-01-21 | MIT |
| 51 | toptesi | Politecnico di Torino (IT) | DTX (CTAN) | report | FILE-ENV (`ThesisTitlePage` env with setters inside) | 1arg (`\titolo`, `\candidato`, `\relatore`) | shipped (examples) | any (Lua/Xe examples) | biber (option) | – (CTAN 6.4.07) | CTAN | LPPL-1.3c |

51 rows (50 LaTeX + 1 Typst). Extra MIT data point: `bkettle/mit-thesis-template` (17 stars, 2023,
unofficial Overleaf copy of the *old* `mitthesis.cls`, `\def\maketitle{\begin{titlepage}…`, pdfLaTeX,
bibtex) — superseded by the CTAN class in row 43.

Not covered (searched, nothing usable found on GitHub): RWTH Aachen (official templates are behind the
RWTH intranet / Overleaf portal — unverified), Uni Heidelberg, Uni Freiburg, HAW Hamburg (only Typst
templates found: `LasseRosenow/HAW-Hamburg-Typst-Template`, 26 stars), TU Ilmenau, TU Chemnitz,
TUHH. The official TUM corporate-design templates are distributed via tum.de, not GitHub
(unverified).

## 2. Per-template details

Paths are relative to the repo root.

### 1. TUDa-CI (TU Darmstadt) — `tudace/tuda_latex_templates`, CTAN `tuda-ci`
- Files read: `tex/tuda-ci.dtx`, `tex/tuda-ci.ins`, `tex/tudafonts.dtx`, `examples/DEMO-TUDaThesis.tex`, `latexmkrc`.
- Real DTX class `tudapub`; `class=report|book|article` picks the KOMA base (`\LoadClass{\g_ptxcd_pub_class_tl}`); options are l3keys (`thesis={type=bachelor}`, `accentcolor=9c`, …).
- Title: `\maketitle` of the class. Metadata: `\title \subtitle \author` plus one-arg `\reviewer \department \institute \group \submissiondate \examdate`. `\department{ce}` expands a shorthand.
- Logos: `logofile` key (default `tuda_logo`); logos not in the repo (licence-excluded), `tools/logo-installation/` builds a bundle.
- Engine: `tudafonts.dtx` loads fontspec under Lua/Xe; repo `latexmkrc` uses `$pdf_mode = 4` (LuaLaTeX). PDF/A via `\SetKeys[document/metadata]` before `\documentclass` in the demo.
- Front matter: `\affidavit[…]` (disabled by default in demo), abstract env redefined.
- Languages: de/en (babel). This is the case Shale already supports.

### 2. TUD-Script (TU Dresden) — `tud-cd/tudscr`, CTAN `tudscr`
- Files read: `source/tudscr-misc.dtx` (`\LoadClass{\TUD@Class@KOMA}`), `tudscr-title.dtx`, `tudscr-frontmatter.dtx`, `tudscr-fonts.dtx`, `source/doc/examples/thesis.tex`.
- DTX classes `tudscrreprt/tudscrbook/tudscrartcl` on KOMA. `\maketitle` redefined (`\let\maketitle\tud@maketitle`).
- Metadata: `\faculty \department \institute \chair \subject{master} \graduation[M.Sc.]{Master of Science} \supervisor \professor \matriculationyear` — one-arg (plus optional arg); `\matriculationnumber \dateofbirth \placeofbirth` are written *inside* `\author{…}` per author. In the example they come after `\begin{document}`.
- Front matter commands: `\declaration`, `\nextdeclaration`, `\confirmation` (statutory declaration), abstract environment.
- Engine: fontspec branch + Type1 Open Sans for pdfLaTeX. Last release 2022 — predates the 2023 TUD corporate design (hence row 3).

### 3. netd-tud teaching-thesis-template (TU Dresden) — `netd-tud/teaching-thesis-template`
- Files read: `thesis.tex`, `preamble.tex`, `tex-sty/tud-titlepage.sty`, `tex-sty/tud-thesis.sty`.
- Stock `scrbook` + own packages: `tud-titlepage.sty` does `\renewcommand{\maketitle}{\begin{titlepage}…}` and one-arg setters `\faculty \department \institute \chair \matriculationnumber \thesis \graduation[…]{…} \supervisor \professor \reviewer \email` (same names as tudscr). `tud-thesis.sty` provides `\confirmation` (bilingual declaration).
- Logos shipped: `figures/tud-logo-{blau,weiss}-{de,en}.pdf`. Engine: iftex, T1 only under pdfTeX. biber, minted, svg (both need shell-escape).
- **This is exactly the "example + wrapper .sty" structure Shale needs**; it is ready for Shale's model as is.

### 4. tum-thesis-latex — `fwalch/tum-thesis-latex`
- Files: `main.tex` (scrbook), `settings.tex` (inputenc/T1, babel ngerman+american, biblatex biber), `pages/cover.tex`, `pages/title.tex`, `pages/acknowledgments.tex`, `pages/abstract.tex`, `.latexmkrc`.
- Metadata are `\newcommand*{\getTitle}{…}`, `\getTitleGer`, `\getAuthor`, `\getDoctype`, `\getSupervisor`, `\getAdvisor`, `\getUniversity`, `\getFaculty` (zero-arg variables), read by two `titlepage` files that `main.tex` `\input`s in the body. No `\maketitle`.
- Logos: `logos/` is empty in the repo; `crop-logos.sh` expects the user to download TUM logos.
- Declaration: in the title page/acknowledgments set (not separate). Languages en + de title.

### 5. TTT (TUM) — `MichaelGrupp/TTT`
- `settings.tex`: `\documentclass[pdftex,…]{scrbook}`, inputenc utf8, T1, natbib+plain; variables `\faculty \authorname \supervisor \advisor`.
- `TUM_Thesis_Template.tex` includes `titlepage`, `titlepage_inside`, `content/EidesstattlicheErklaerung`, `content/Abstract`. Logos shipped in `images/TUM_Logos/`.
- The global `pdftex` option is a pdfLaTeX driver option (risk under LuaLaTeX, unverified).

### 6. TUM CSE — `waltsims/TUM_Thesis_Template_CSE`
- `main.tex` (scrbook) inputs `components/cover`, `components/titlepage`, `components/disclaimer`, `acknowledgements`, `abstract`; metadata via `\def\author{…}`, `\def\examinerOne{…}` in a settings file (overwrites LaTeX's `\author`!).
- glossaries with xindy (`\makeglossaries`), minted (shell-escape), bibtex plain. Logos shipped (`styles/tum_logo.pdf`, `cse_logo.pdf`).

### 7. scientific-thesis-template (Uni Stuttgart, Uni Hamburg, TH Ulm) — `latextemplates/scientific-thesis-template`
- Files: `main-english.tex`, `main-german*.tex`, `scientific-thesis-cover.sty` (also on CTAN), `.latexmkrc`.
- scrbook. Metadata are **package options** of `scientific-thesis-cover`: `\usepackage[title={…},author={…},type=bachelor,institute=iaas,course={…},examiner={…},supervisor={…},startdate={…},enddate={…},language=english,ustutt]{scientific-thesis-cover}` (kvoptions `\DeclareStringOption`). Title page: `\Titelblatt` (alias `\Coverpage`) in the body; the sty also produces the declaration ("Erklärung"/"Declaration").
- Engine: `\ifpdftex`/`\ifluatex` branches throughout; `.latexmkrc` runs `lualatex`. minted (shell-escape), biber with `natbib=true`. The main files are ~1200-line generated preambles (generator-latex-template).

### 8. uni-stuttgart-dissertation-template — `latextemplates/uni-stuttgart-dissertation-template`
- `thesis-example.tex` (scrbook), `ustutt-titlepage.sty`: `\renewcommand\maketitle{…\begin{titlepage}…}` then `\global\let\maketitle\relax`; one-arg setters `\firstexaminer \secondexaminer \dateofexamination \placeofbirth \faculty …` (`\def\@x{#1\xspace}`). `\title \author \date` standard.
- Front matter after `\maketitle`: `\chapter*{Kurzfassung}`, `\chapter*{Abstract}`. Engine: iftex branches, LuaLaTeX recommended. Logos shipped (`ustutt-logo.pdf`). Fits Shale's model directly.

### 9. tudothesis (TU Dortmund) — `maxnoe/tudothesis`
- `tudothesis.cls`: `\LoadClass[…]{scrbook}`; `thesis.tex`: `\author \title \date` + one-arg `\birthplace \chair \division \thesisclass \submissiondate \firstcorrector \secondcorrector`; `\frontmatter\maketitle\tableofcontents\mainmatter`.
- Declaration: `\includepdf{content/Eidesstattliche_Versicherung.pdf}` (a PDF form, not typeset). Abstract: `content/00_abstract.tex` (Kurzfassung + English via `foreignlanguage`).
- Engine: main.tex loads fontspec and unicode-math → LuaLaTeX/XeLaTeX only. Logos shipped (`logos/tu-logo.pdf`). Near-ideal for Shale.

### 10. tubslatex (TU Braunschweig) — `ejoerns/tubslatex`
- DTX bundle (`report/src/tubs*.dtx`, `tubs.ins`): classes `tubsreprt`, `tubsartcl`, `tubsposter`, `tubsdoc`; `tubstitlepage.dtx` redefines `\maketitle[opts]` with xkeyval keys; extra setters `\titleabstract{…}`.
- Engine: every source has `\usepackage[utf8]{inputenc}`, no fontspec branch (pdfLaTeX era; Nexus font). Last commit 2015 → stale; official TU BS distribution now elsewhere (unverified).

### 11. TUB_PhDThesisTemplate (TU Berlin) — `holgern/TUB_PhDThesisTemplate`
- `Classes/PhDthesisTUB.cls` (fork of kks32, `\LoadClass{book}`), `thesis-info.tex` with `\title \subtitle \author` + setters, `\maketitle` redefined; `pdfa1` option, glossaries (`\makeglossaries`), option `biblatex`.
- Engine: `\ifluatex`/`\ifxetex` branches; `Classes/glyphtounicode.tex` (pdfTeX primitives, presumably guarded — unverified).

### 12. KIT OS group — `KIT-OSGroup/thesis-template`
- `thesis.tex` (babel american+ngerman, `inputenc[ansinew]`), `thesis_title.tex`: `\title \author \thesistype{ba} \primaryreviewer \advisor{…}{…} \thesisbegindate \thesisenddate`, then `\maketitle` (redefined in `styles/kitthesiscover.sty` to `\ktc@maketitle`).
- **pdfTeX-only**: the cover uses `\pdfximage`/`\pdfrefximage` directly (guarded only by `\pdfoutput`) → expected to fail under LuaLaTeX (reading, not tested). Logos `fig/kit_logo_*` shipped. One-arg setters otherwise fit Shale.

### 13. kit-thesis-template — `eneoli/kit-thesis-template`: Typst (7 `.typ` files). Skipped.

### 14. lmu-thesis-latex — `changkun/lmu-thesis-latex`
- `thesis.tex` (`article`) + `theme/lmuthesis.sty` (T1, `inputenc[latin1]`, babel english). `\title \author` standard; `\abstract{…}` redefined as one-arg setter, printed by `\makeabstract`; no `\maketitle`. Logo shipped. latin1 input encoding conflicts with Shale's UTF-8 output.

### 15. Hochschule Hannover — `hsh-elc/hsh-latex-templates` (reference case)
- `Abschlussarbeiten/main.tex` (scrbook) inputs `tex/prae-com`, `tex/prae-dev`; metadata macros with optional label + several args: `\VerfasserIn[Verfasserin]{Jane Doe}{1234567}`, `\BetreuerInA[Erstprüferin]{…}`, `\DatumAbgabe[…]{…}`, `\LogoHsH{1}`, read back as `\VarVerfasserIn`, `\VarDatumAbgabe`. Title page `0a-Titelblatt.tex` (`titlepage` env) input in body; Eigenhändigkeitserklärung typed inline in main.tex; `0c-Kurzfassung`. glossaries + makeglossaries. Logos shipped. inputenc+T1.

### 16. HagenbergThesis — `Digital-Media/HagenbergThesis`, CTAN `hagenberg-thesis`
- `dev/texmf/tex/latex/hagenberg-thesis/hgbthesis.cls` (`\LoadClass[…]{book}`, `\DeclareKeys` class options `type=master,degree=…,theme=…,language=…,aiusage=…`), themes `hgbtheme-*.sty` with logos, `hgbbib.sty`, `hgbpdfa.sty`.
- `\maketitle` redefined to `\hgb@MakeFrontPages` (title page, affidavit, etc. per theme). Metadata: `\title \subtitle \author \programname \placeofstudy \advisor \license{cc}` and 3-arg `\dateofsubmission{2027}{07}{01}`; in the tutorial they come **after** `\begin{document}`.
- Engine: T1 fontenc unless already set, iftex; tutorial documents lualatex+fontspec. biblatex (`\bibliography{…}` via hgbbib). Abstract/Kurzfassung as `\include{front/…}`.

### 17. vutinfth (TU Wien Informatics) — `AndreasPK/vutinfth` (3-star mirror of the official class)
- `vutinfth.dtx` (`\LoadClass[a4paper,11pt]{memoir}`, babel english+naustrian), `example.tex`.
- Metadata are multi-argument: `\setauthor{Pretitle}{Name}{Posttitle}{female}`, `\setadvisor{…}{…}{…}{male}`, `\setdate{01}{01}{2001}`, `\settitle{EN}{DE}`, `\setsubtitle{EN}{DE}`, `\setcurriculum{EN}{DE}`, `\setthesis{bachelor}`. Title: `\addtitlepage{naustrian}` and `\addtitlepage{english}` in the body; environments `danksagung*`, `acknowledgements*`, `kurzfassung`, `abstract`, statement page.
- Engine: T1+inputenc in example; bibtex alpha; glossaries. The official upstream (TU Wien informatics website) was not checked — unverified whether newer.

### 18. abschlussarbeit-tuwien-physik — `joerg/abschlussarbeit-tuwien-physik`
- scrreprt; `titlepage.tex` defines xkeyval keys (`autor, titel, institut, prof, adresse, typ, schwarzweisslogo`) set via `\AbschlussarbeitTUWienPhysikTitlePage{key=val}`, printed by `\titlePageTUWienPhysik`. pdfx (PDF/A-2u) with `\ifpdf` glyphtounicode block. biber. Logos shipped.

### 19. ethz-thesis — `tuxu/ethz-thesis`
- classicthesis-derived scrreprt (A5). `\newcommand{\myTitle}…\myName…\myDissNumber…`; `thesis.tex` includes `frontbackmatter/dirtytitlepage`, `titlepage`, `titleback`, `abstract`, `acknowledgments`. biber with several `\addbibresource`. inputenc+T1. No logos.

### 20. ZHAW / MastersDoctoralThesis — `matteodelucchi/ZHAW_thesis-template`
- `MastersDoctoralThesis.cls` v1.7 (LaTeXTemplates.com "Masters/Doctoral Thesis", Vel & Johannes Böttcher; class LPPL-1.3c; `\LoadClass{\baseclass}` = book). This class is also the one behind the very popular Overleaf "Masters/Doctoral Thesis" gallery template (not separately verified on Overleaf).
- Metadata: one-arg `\thesistitle \author \degree \supervisorA \supervisorAmail … \university \universitygerman \department \group \keywords` (stored as `\ttitle`, `\authorname`, `\univname`, …). Title page is a hand-written `Front/titlepage.tex` (`titlepage` env) using those macros; `\include{Front/declaration}`, `Front/abstract`, `Front/acknowledgements`, plus `Appendices/DeclarationOfOriginalityZHAW`.
- Engine: `\ifxetex` → fontspec else inputenc/T1. biber. Logo shipped.

### 21. uzh-ma-thesis — `maehr/uzh-ma-thesis`
- `ma-thesis.tex` (report) with a typed `titlepage` env; `template/template.tex` is a pandoc LaTeX template (`$title$`, `$author$`, `$abstract$`). Logo shipped. Interesting because a pandoc template is structurally close to what Shale generates.

### 22. HSLU — `HSLU-BaumannWicki/BA-Dokumentation-Template`
- scrreprt, `include/HSLU_Preamble.tex` holds a typed `titlepage` (textpos logos) and "Eidesstattliche Erklärung". pdfx a-2b, glossaries, biber, inputenc. Logos shipped (`img/HSLU_Logo.pdf`).

### 23. iodhbwm (DHBW Mannheim) — `faltfe/iodhbwm`, CTAN `iodhbwm`
- `source/iodhbwm.cls` (pgfkeys class options, `\LoadClass[…]{\iodhbwm@cls@baseclass}` — KOMA by default), `source/iodhbwm-templates.sty`, i18n files.
- Metadata: `\dhbwsetup{author=…, thesis type=PA, thesis title=…, student id=…, institute=…, institute logo=…, supervisor=…, processing period=…}` (keys with spaces). Title: `\dhbwtitlepage` or class option `load-dhbw-templates` with `\dhbwprintintro` (title page + declaration + abstract); falling back to `\maketitle` is documented (`doc/iodhbwm.tex` l. 630).
- Engine: examples use T1 (pdfLaTeX); biblatex style chosen via class keys. DHBW logo shipped (`source/dhbw-logo.png`).

### 24. dhbw-stuttgart-template — `knopfm/dhbw-stuttgart-template`
- `config/header.tex`: `\documentclass[…]{scrreprt}`, inputenc/T1 ("needed by the pdflatex engine"), `\newcommand{\documentType}{T3\_3300}` etc., `\title{\documentTitle}`; `main.tex` inputs `resources/cover`, `resources/declaration`, `resources/abstract`, `resources/acronyms` (acronym package). biber. Logos `logo-dhbw.png`, `logo-company.png`. `.gitlab-ci.yml` present.

### 25. dhbw-heidenheim — `StefanFW/dhbw-heidenheim-latex-template`
- Same family as 24 (`ads/header.tex`, `ads/cover.tex`, `ads/declaration.tex`, `lang/de.tex`/`en.tex` phrases, `\documentLanguage{de}` switch). scrreprt, inputenc/T1, biber.

### 26. FH Aachen — `SvenHinz/FH-Aachen-LaTeX-Vorlage-Abschlussarbeit`
- `000arbeit.tex` (scrreprt) includes `doc/001titelblatt.tex` whose values (department, title, name, Matr.-Nr.) are **typed directly** into the titlepage; `doc/002erklaerung`, `003danksagung`. natbib + unsrtdin, acronym. Logo `pic/FHAC.jpg` (+ a `_ohne_logo` variant).

### 27. Hochschule München — `Simon-Hi5/Hochschule-Muenchen-LaTeX-Template`
- `Thesis-Template.tex` (book) inputs `frontmatter/titlepage.tex` (typed values, `figures/logos/hm-logo.png`), `acknowledgments`, `abstract` (Kurzfassung). glossaries-extra + makeglossaries, biber ieee. T1.

### 28. hm-seclab — `hm-seclab/thesis-template`
- Own class `zyx/mythesis.cls` (`\LoadClass[…]{scrbook}`, `\title{\myTitle}`, `\makeglossaries`), `zyx/preface.tex` builds title page + optional acknowledgments/abstract via `subfiles`; language switch `\mylanguage`. inputenc/T1, biber ieee, glossaries.

### 29. HTW Berlin — `grollk/htw-berlin-latex-thesis-template`
- `resources/settings.tex` (scrreprt, **fontspec**) `\include`s `resources/titlepage.tex`, which defines one-arg setters `\gradeType \program \facultyNr \firstExaminer \secondExaminer` and `\renewcommand*{\maketitle}{\begin{titlepage}…`; `main.tex` sets `\title \author` and calls `\maketitle`. Logos must be unpacked by the user (`resources/images/unpack HTW logos zip file here`). No bibliography configured.

### 30. Hochschule Mainz — `jonasbingel/HSMZ-Thesis-Template`
- `Arbeit.tex`/`Expose.tex` (scrreprt), `00_Allgemein/Packages.tex` (inputenc/T1, biblatex, minted), variables `\titel \autorVorname \autorNachname \matrikelnr \betreuer \datumAbgabe \datumAblaufSperrvermerk`; `01_Vortext/02_Titelseite.tex`, `03_Erklaerung`, `04_ManagementSummary`. minted needs shell-escape.

### 31. H-BRS — `MartinX3-EducationOrganization/hbrs-latex-vorlage-arbeit`
- `Arbeit.tex` (scrreprt) + `aussehen/hbrs-inf.sty` (**fontspec**, babel ngerman, biblatex iso-authoryear); `abschnitte/Deckblatt.tex` uses variables `\ThesisTitle \ThesisSubjectType \ThesisStudyCourseGerman …`; `abschnitte/Erklaerung.tex`, `Zusammenfassung.tex`; glossaries acronyms. Logo `bilder/fhlogo`.

### 32. TH Nürnberg — `StefanMacke/latex-vorlage-masterarbeit`
- `Masterarbeit.tex` (scrreprt), `Packages.tex` (`inputenc[latin1]`, T1, natbib+natdin, nomencl, makeindex), variables `\titel \autor \matrikelnr`, `Deckblatt.tex`. Bundles copies of `floatflt.sty` and `chngcntr.sty`. Stale (2015).

### 33. latex-vorlage-diplom-bachelor-masterarbeiten — `maknesium/…`
- `src/Hauptdatei.tex` (scrartcl!), `latex_einstellungen/deckblatt.tex` (placeholders like `Fachhochschule <Name>` typed + `\titleDocument`), `erklaerung.tex` (Eidesstattliche Erklärung), nomencl, bibtex alphadin, inputenc/T1. Sections, not chapters.

### 34. LaTeX-Master-Vorlage — `a-czyrny/LaTeX-Master-Vorlage`
- `resources/settings/settings.tex` (scrbook, T1, babel ngerman, biblatex biber, glossaries); `main.tex` `\include{titles/graduation}` which defines `\firstExaminer \subTitle \matrikelnr \submitDate …` (`\gdef\@x{#1}`) and `\renewcommand*{\maketitle}{\begin{titlepage}…}`; `\title \author` then `\maketitle`; `chapter/Eigenstaendigkeitserklaerung`, `chapter/Abstract`.

### 35. Clean Thesis — `derric/cleanthesis`, CTAN `cleanthesis`
- `cleanthesis.sty` (keyval options `\setkeys[ct]{cthesis}{…}`, `configurebiblatex=true`), `example-thesis/my-thesis.tex` (scrreprt), `my-thesis-setup.tex` (`\newcommand{\thesisTitle}…\thesisName…\thesisFirstReviewer…`), `content/titlepages.tex` (two `titlepage` envs), `content/abstract`, `acknowledgement`, `declaration`.
- Engine: `\usepackage{inputenc}` in setup; sty itself engine-neutral (unverified for fonts: uses Lato/Bera via packages). Last commit 2021.

### 36. classicthesis — CTAN `classicthesis` 4.8 (2025-02-02), upstream `bitbucket.org/amiede/classicthesis`
- Read from CTAN: `classicthesis.sty` (kvoptions `\DeclareStringOption[a4]{paper}`, style, …), `ClassicThesis.tex` (scrreprt), `classicthesis-config.tex` (`\newcommand{\myTitle}…\myName…\myProf…\myFaculty…`), `FrontBackmatter/Titlepage.tex`, `Declaration.tex`, `Abstract.tex` (Abstract + Zusammenfassung via `otherlanguage`).
- Engine: sty branches on `ifxetex`/`ifluatex` and uses fontspec+OpenType for UTF-8 engines; config loads `inputenc` only under pdfTeX (unverified condition). Bibliography: biblatex with `natbib=true`; acronym package. The GitHub repo `classicthesis/classicthesis` is a stale 2013 fork (uses natbib+plainnat).

### 37. kaobook — `fmarotta/kaobook`
- `kaobook.cls` (`\@baseclass` = scrbook), `kao.sty`, `kaobiblio.sty` (biblatex backend=biber), `kaorefs.sty`, `kaotheorems.sty`; examples `examples/minimal_book/main.tex`: `\titlehead \title[short]{…} \author[short]{…} \date \publishers \uppertitleback \lowertitleback \dedication`, then `\frontmatter … \maketitle`.
- Margin-note layout (`\pagelayout{wide|margin}`, `\setchapterstyle{kao}`); minted optional (`\ifminted`). Engine: iftex, fontspec when Xe/Lua. Not on CTAN (CTAN API "not found"). Thesis-agnostic (no declaration).

### 38. latex-mimosis — `Pseudomanifold/latex-mimosis`
- `mimosis.cls` (`\LoadClass[…]{scrbook}`, babel ngerman/french/english, glossaries acronym style), `Thesis.tex`: `\title \subtitle \author`, `\include{Sources/Title}` — a `titlepage` file that prints `\@title`, `\@subtitle`, `\@author` (so standard metadata, but no `\maketitle`); alternative files `Title_Dissertation_Heidelberg.tex`, `Title_Dissertation_ETH_Zurich.tex`. biber. iftex/fontspec aware. Most-starred generic template found (1244).

### 39. novathesis — `joaomlourenco/novathesis`
- `novathesis.cls` + `novathesisFiles/StyFiles/nt-*.sty` (`\LoadClass{memoir}` in `nt-packages.sty`, l3keys), config in `0-Config/*.tex`, `template.tex` is only `\ntprint{frontcover}\ntprint{frontmatter}\ntprint{mainmatter}\ntprint{backmatter}\ntprint{backcover}`.
- Metadata: `\ntsetup{school = other/novathesis}`, `\nttitle(main,en){…}`, `\ntdepartment(en){…}`, `\ntauthorname(m){Full}{Short}`, `\ntaddperson{adviser}(a,f){Name}[title=…,position=…]` — parenthesised language/gender args + kv. Covers ~30 schools incl. a `other/huberlin` variant.
- `latexmkrc`: LuaLaTeX default; TEXINPUTS set to `novathesisFiles/StyFiles`. Heaviest template in the set.

### 40. Dissertate — `suchow/Dissertate`
- `assets/latex-base/Dissertate.cls` (`\LoadClass[…]{book}`), per-school `assets/schools/<School>/style.sty` each `\renewcommand{\maketitle}` and one-arg setters (`\yalecommitteechair`, `\uclachair`, …); `frontmatter/personalize.tex`: `\title \author \advisor \committeeInternalOne … \degree \field \degreeyear \degreeterm \degreemonth \department`.
- bibtex (apalike2, IEEEtran per school). No fontspec in the base class.

### 41. Cambridge CUED — `kks32/phd-thesis-template`
- `PhDThesisPSnPDF.cls` (`\LoadClass{book}`, `\renewcommand{\maketitle}`), `thesis-info.tex`: `\title \subtitle \author \dept \university \crest{\includegraphics…} \degreetitle \college \subject \keywords`. `thesis.tex`: `\frontmatter\maketitle` then `\include{Declaration/declaration}`, acknowledgement, abstract; nomenclature (`\printnomenclature`, makeindex step).
- Engine: ifxetex/ifluatex branches; option `times`; bibtex+natbib or biblatex by class option. Crest must be supplied by the user.

### 42. UCL — `UCL/ucl-latex-thesis-templates`
- `ucl_thesis.cls` (`\LoadClass{book}`, `\renewcommand\maketitle` then `\let\maketitle\relax`), `Preamble.tex` (included *after* `\begin{document}`): `\title \author \department`, `\maketitle`, `\makedeclaration`, `abstract`, `impactstatement`, `acknowledgements` environments.
- `LinksAndMetadata.tex`: `\usepackage[pdftex,hidelinks]{hyperref}` — explicit pdftex driver (risk under LuaLaTeX, unverified). bibtex unsrt.

### 43. mitthesis (MIT, official) — CTAN `mitthesis` 1.25 (2026-11-23)
- Read: `mitthesis.cls` (`\LoadClass[12pt,openright]{report}`), `MIT-thesis-template/MIT-Thesis.tex`, `fontsets/*.tex`.
- Metadata after `\begin{document}`: `\title{…}`, multi-arg `\Author{name}{dept}`, `\Degree{…}{dept}`, `\Supervisor{name}{title}[dept]`, `\Acceptor{…}{…}{…}{}`, `\DegreeDate{June}{1876}`, `\ThesisDate{…}`, `\Reader{…}{…}{…}`; then `\maketitle`, `abstract` env, `\include{acknowledgments}`, `biography`.
- Engine: fontset files branch `\ifpdftex` vs unicode-math; tagged-PDF ready (`\tagpdfsetup`). biblatex `ext-numeric-comp`. No logo.

### 44. Stanford — `dcroote/stanford-thesis-example`
- `main.tex` (`report`) + `suthesis-2e.sty` (`\let\maketitle=\titlep`); `\title \author \dept \principaladviser \firstreader \secondreader`; front matter through `\beforepreface … \include{preface_abstract} … \afterpreface` (title, copyright, signature pages, ToC are emitted by `\beforepreface/\afterpreface`). bibtex unsrt.

### 45. EPFL — `glederrey/EPFL_thesis_template`
- `my_thesis.tex` + `settings/settings_epfl_template.tex` (book, T1/inputenc, natbib, babel french/german/english); `content/head/titlepage.tex` is typed (explicitly "temporary title page" replaced by EPFL print shop), `acknowledgements`, `abstracts`. Logo `assets/epfl` shipped (unverified licence).

### 46. ut-thesis (Toronto) — `jessexknight/ut-thesis`, CTAN `ut-thesis`
- `ut-thesis/ut-thesis.dtx` → `.cls` (`\LoadClass{book}`), one-arg `\author \title \degree \department[]{} \gradyear \copyrighttext`; `\renewcommand*{\maketitle}`; environments `abstract`, `dedication`, `acknowledgements`. No bibliography or engine choices imposed. Cleanest small class in the set.

### 47. yathesis (France) — `dbitouze/yathesis`, CTAN `yathesis`
- `tex/latex/yathesis/yathesis.cls` (`\LoadClass{book}`, pgfkeys `\yadsetup`), canevas `doc/latex/yathesis/french/exemples/canevas/a-plat/these.tex`: `\author[]{first}{last}`, `\title[secondary]{main}`, `\date{d}{m}{y}`, `\institute[logo=,url=]{…}`, `\supervisor[affiliation=]{first}{last}`, `\referee…`, `\committeepresident…`; `\maketitle[opts]` redefined; warns if `\maketitle` unused. Many metadata after `\begin{document}`.

### 48. thuthesis (Tsinghua) — `tuna/thuthesis`, CTAN `thuthesis`
- `thuthesis.dtx` (`\LoadClass[…]{ctexbook}`, kvoptions `\SetupKeyvalOptions`), `thusetup.tex`: `\thusetup{title = {…}, title* = {…}, …}`; `thuthesis-example.tex`: `\maketitle\frontmatter\tableofcontents\mainmatter`. Needs ctex/Chinese fonts (XeLaTeX recommended upstream, unverified for Lua). Included as the most-starred thesis class worldwide (5499).

### 49. adsphd (KU Leuven) — `wannesm/adsphd`
- `adsphd.cls` (`\LoadClass{report}`, kvoptions `faculty=…,department=…`, `\RequirePackage{fontspec}`), `thesis.tex`: `\title \subtitle`, two-arg `\author{Your}{Name}`, `\supervisor{name}{affil}`, `\jurymember{…}{…}`, … then `\maketitle` (cover + title page) and `\includeabstract{…}` style includes. README: LuaLaTeX by default. Logos shipped (`image/KULEUVEN_LOGO_2012.pdf`).

### 50. Oxford MSD — `zhiyhu/thesis-template-Oxford`
- `main.tex` (report, natbib), `titlepage.tex` typed (Main title / Name / College …), crest image `oxford` shipped. Abstract/acknowledgements/declaration as `\chapter*` in main.tex. Low stars; the classic `ociamthesis.cls` was only found in unmaintained forks.

### 51. toptesi (Politecnico di Torino) — CTAN `toptesi` 6.4.07
- Read: `toptesi.dtx` (`\LoadClass{report}`), `toptesi-example-luatex.tex`. Class option `tipotesi=magistrale`; title page is an environment in the body: `\begin{ThesisTitlePage}* \ateneo{…} \logosede{logouno} \titolo{…} \sottotitolo{…} \corsodilaurea{…} \candidato{…} \relatore{…} \sedutadilaurea{…} \retrofrontespizio{…} \end{ThesisTitlePage}`; then `\sommario`, `\ringraziamenti`, `\indici`. LuaLaTeX example with fontspec + unicode-math; biblatex via option.

## 3. Analysis

### 3.1 Structure counts (50 LaTeX templates; Typst excluded)

| Structure | Count | Templates |
|---|---|---|
| Real class or package usable on its own (CLS/DTX, incl. CTAN sty+template bundles) | 22 | TUDa-CI, TUD-Script, tudothesis, tubslatex, TUB, Hagenberg, vutinfth, MDT/ZHAW, iodhbwm, cleanthesis*, classicthesis*, kaobook, mimosis, novathesis, Dissertate, CUED, UCL, mitthesis, suthesis*, ut-thesis, yathesis, thuthesis, adsphd, toptesi (\* = package on a stock class) |
| of which built from `.dtx` | 9 | TUDa-CI, TUD-Script, tubslatex, vutinfth, ut-thesis, yathesis, thuthesis, toptesi (+ Hagenberg/iodhbwm ship `.cls` directly) |
| Example document + own wrapper `.sty`/`.cls` holding title & metadata | 8 | netd-tud, uni-stuttgart-diss, scientific-thesis, KIT-OS, LMU, hm-seclab, H-BRS, (HTW/LaTeX-Master-Vorlage put the same code in an `\input` file) |
| Pure example document (stock KOMA/book + `\input` files) | 19 | TUM×3, HsH, TU Wien Physik, ETH, UZH, HSLU, DHBW×2, FH Aachen, HM, HTW, HS Mainz, TH Nürnberg, maknesium, LaTeX-Master-Vorlage, EPFL, Oxford |

(Some templates count in two rows: e.g. ZHAW is an MDT class *plus* a hand-written title file.)

Metadata mechanism (primary):

| Mechanism | Count | Examples |
|---|---|---|
| std `\title\author\date` + one-arg setters (`\supervisor{…}`) | 20 | TUDa, TUD-Script, netd-tud, uni-stuttgart, tudothesis, TUB, KIT-OS, HTW, LaTeX-Master, Hagenberg (mostly), MDT, kaobook, mimosis, Dissertate, CUED, UCL, suthesis, ut-thesis, toptesi, LMU |
| zero-arg variables `\newcommand{\myTitle}{…}` read by a title file | 13 | TUM×3, ETH, classicthesis, cleanthesis, DHBW×2, HS Mainz, H-BRS, TH Nürnberg, hm-seclab, maknesium |
| key-value (setup command, package/class options, pgfkeys, xkeyval) | 7 | scientific-thesis (package options), iodhbwm (`\dhbwsetup`), thuthesis (`\thusetup`), novathesis (`\ntsetup` + `(…)` args), yathesis (`\yadsetup`), TU Wien Physik (`\setkeys`), tubslatex (`\maketitle[keys]`) |
| multi-argument setters | 6 | HsH, vutinfth, mitthesis, adsphd, yathesis, novathesis (plus Hagenberg's 3-arg date) |
| typed directly into the title file | 7 | FH Aachen, HM, HSLU, EPFL, Oxford, UZH, (maknesium partly) |

Title page mechanism: class `\maketitle` (possibly redefined by class/sty) 24; separate file or
environment in the body 22; own command (`\dhbwtitlepage`, `\Titelblatt`, `\addtitlepage`,
`\ntprint`, `\makeabstract`) 5.

Engine: 21 pdfLaTeX-oriented (inputenc/T1 without branch), 21 engine-agnostic via `iftex` branches,
8 LuaLaTeX/XeLaTeX-first (fontspec unconditional: tudothesis, HTW, H-BRS, adsphd, novathesis default,
thuthesis, toptesi-luatex example, mitthesis fontsets). Bibliography: 30 biblatex+biber, 18 BibTeX
(natbib/plain/DIN styles), 2 none.

Extras that need tooling beyond lualatex+biber: glossaries/makeglossaries (HsH, TUM CSE with
**xindy**, HM, hm-seclab, TUB, H-BRS, HSLU, LaTeX-Master, mimosis, novathesis), nomencl/makeindex
(CUED, adsphd, TH Nürnberg, maknesium), **minted/svg → shell-escape** (scientific-thesis,
uni-stuttgart, TUM CSE, HS Mainz, netd-tud; kaobook optional), `\includepdf` of a signed declaration
PDF (tudothesis), pdfx/PDF/A (HSLU, TU Wien Physik, Hagenberg, TUB, TUDa via `\DocumentMetadata`-style
keys).

### 3.2 Recurring patterns an adapter format could cover

1. **One-arg field → command** (Shale's current model): enough for ~20 templates once the right
   command names are known. Setters sometimes must come *after* `\begin{document}` (tudscr example,
   Hagenberg tutorial, mitthesis, UCL, yathesis) — they usually also work before it, but Hagenberg
   and mitthesis should be test-compiled both ways (unverified).
2. **Multi-argument / optional-argument setters** (`\VerfasserIn[label]{name}{matnr}`,
   `\Author{name}{dept}`, `\author{first}{last}`, `\setauthor{pre}{name}{post}{gender}`,
   `\dateofsubmission{Y}{M}{D}`): an adapter needs a field spec like
   `command = "\\setauthor", args = ["{pre}", "{name}", "{post}", "{gender}"]` with each slot bound to a
   property, a constant or empty. Optional `[…]` slots are common (`\graduation[M.Sc.]{…}`,
   `\department[]{…}`).
3. **Key-value setup** (`\dhbwsetup{key=val,…}`, `\thusetup{…}`, `\usepackage[title={…},…]{pkg}`,
   `\ntsetup{…}`): one "kv block" kind: `block = "\\dhbwsetup"`, entries `key → property`, with
   brace-protection of values (keys can contain spaces, as in iodhbwm).
4. **Zero-arg variables** (`\newcommand{\myTitle}{…}`): a field kind "define macro" — Shale emits
   `\newcommand{\getTitle}{value}` (or `\renewcommand` when the template predefines it) and the
   template's title file reads it. Covers the 13 "vars" templates without touching their files.
5. **Title page in the body** (`\input{titlepage}`, `\include{FrontBackmatter/Titlepage}`,
   `\begin{ThesisTitlePage}…`, `\Titelblatt`, `\dhbwtitlepage`): instead of hard-coding
   `\maketitle`, the kind needs a `title_sequence` (list of lines emitted after `\begin{document}`),
   default `["\\maketitle"]`; examples: `["\\input{0a-Titelblatt}"]`, `["\\Titelblatt"]`,
   `["\\addtitlepage{naustrian}", "\\addtitlepage{english}"]`, `["\\beforepreface", …]`.
6. **Front matter slots** after the title: declaration (`\affidavit`, `\confirmation`,
   `\makedeclaration`, `\declaration`, or a file), abstract environment, acknowledgements
   environment. A kind could map note properties/sections ("Abstract", "Danksagung") to these
   commands/environments, plus `\frontmatter`/`\mainmatter` switches (memoir/book/KOMA-book only;
   `scrreprt`/`report` have none).
7. **Wrapper package**: for "example document" templates the robust route is what was done for HsH —
   ship a small `.sty` that turns the title file into `\maketitle` and the variables into one-arg
   setters. netd-tud, uni-stuttgart and a-czyrny show this pattern already exists upstream.
8. **Logos**: 30 ship logos in the repo, ~6 require the user to supply them (TUDa, TUM fwalch, HTW,
   CUED crest, yathesis `logo=` keys, iodhbwm `institute logo`), the rest use none. A kind needs a
   "required asset" list with a clear error when missing, and a note that logos are often
   corporate-design-restricted even in permissively licensed repos.

### 3.3 Best first candidates for one-click support

Ranked by: real class or class-like package; one-arg (or simple kv) metadata; `\maketitle` model;
works with LuaLaTeX + biber; permissive licence; maintained; German weighting.

1. **TUD-Script `tudscr`** (TU Dresden) — CTAN/TeX Live class, KOMA, `\maketitle`, one-arg fields
   (`\faculty`, `\supervisor`, `\graduation[…]{…}`), fontspec-capable, LPPL. Only wrinkle: per-author
   fields nested inside `\author{…}` and the 2022 release predates the new CD.
2. **netd-tud teaching template** (TU Dresden, new CD) — same field names as tudscr, `\maketitle` via
   its sty, logos shipped, iftex-clean; drop-in for Shale's model (licence missing → ask authors).
3. **HagenbergThesis** (FH OÖ) — CTAN class, `\maketitle` emits all front pages, mostly one-arg fields
   (plus 3-arg date → needs pattern 2), biblatex, CC-BY; very well maintained (2026-10).
4. **tudothesis** (TU Dortmund) — tiny scrbook class, pure one-arg fields + `\maketitle`, already
   LuaLaTeX-only with biber, MIT licence, logo shipped.
5. **uni-stuttgart-dissertation-template** (`ustutt-titlepage.sty`) — `\maketitle` + one-arg fields;
   iftex-aware, MIT; strip the 1400-line example preamble and keep the sty.
6. **ut-thesis** (Toronto) — minimal CTAN class, one-arg fields, `\maketitle`, environments for
   abstract/acknowledgements; good international smoke test.
7. **kaobook** — generic, popular (1122★), KOMA `\maketitle` with std metadata, biber built in;
   tests Shale against margin-note layouts.
8. **iodhbwm** (DHBW, CTAN) — first key-value candidate (`\dhbwsetup{…}`, title via
   `\dhbwtitlepage`/`\dhbwprintintro`); DHBW has many students. Needs patterns 3 and 5; check its
   pdfLaTeX-only assumptions under LuaLaTeX first.
9. **mitthesis** (MIT official, CTAN, 2026) — multi-arg setters (pattern 2) but otherwise
   `\maketitle` + biber + LuaLaTeX-ready fontsets; good proof that multi-arg support works.
10. **scientific-thesis-cover** (Uni Stuttgart / Uni Hamburg / TH Ulm, CTAN sty) — package-option
    key-value metadata + `\Titelblatt`; LuaLaTeX by default; 0BSD licence.

Honourable mentions: classicthesis (huge reach, but variables + title file → needs pattern 4 or a
wrapper; GPL-2.0+), latex-mimosis (1244★, std metadata but title as file), MastersDoctoralThesis
(Overleaf favourite, one-arg fields but hand-written title file), CUED/kks32 (905★, `\maketitle` +
one-arg; crest must be user-supplied).

### 3.4 What would break under LuaLaTeX or under "\maketitle then body"

From reading the sources (no compile run):

- **pdfTeX primitives**: KIT OS group (`\pdfximage` in `kitthesiscover.sty`) — expected to fail;
  TU Wien Physik / scientific-thesis / TUB guard `\pdfgentounicode`/glyphtounicode with `\ifpdf`/
  `\ifpdftex` (ok).
- **Explicit pdftex driver options**: TTT `\documentclass[pdftex,…]`, UCL
  `\usepackage[pdftex,hidelinks]{hyperref}` — risky under LuaLaTeX (unverified; modern hyperref/graphicx
  may tolerate it). Old mitthesis copy (`bkettle`) uses `lgrind.sty`.
- **Non-UTF-8 input**: `inputenc[latin1]` (LMU, TH Nürnberg), `inputenc[ansinew]` (KIT) — Shale writes
  UTF-8; under LuaLaTeX inputenc is ignored, but the files themselves are Latin-1 encoded.
- **inputenc/T1 without branch** (21 templates): harmless under LuaLaTeX (inputenc is a no-op, T1 with
  Latin Modern works), but fonts change (no fontspec) — cosmetic, not fatal.
- **Chinese/ctex** (thuthesis) needs CJK fonts; Typst (KIT) is out of scope.
- **Shell-escape**: minted/svg templates fail without `--shell-escape`; Shale would need an opt-in.
- **makeglossaries/xindy/makeindex/nomencl** steps: Shale's lualatex+biber pipeline would print empty
  glossaries/nomenclature (not an error, but missing lists).
- **"\maketitle then body" mismatches**:
  - Title page as body file/env (22 templates) → Shale's fixed `\maketitle` prints the stock KOMA/book
    title (or nothing) instead of the institution's page; needs `title_sequence` (pattern 5) or a
    wrapper sty.
  - Classes whose `\maketitle` is not the right entry point: novathesis (`\ntprint{…}` drives the whole
    document, `\maketitle` unused), vutinfth (`\addtitlepage{lang}`), iodhbwm (`\dhbwtitlepage`),
    Stanford (`\beforepreface…\afterpreface` must bracket the front matter and emits the ToC itself →
    Shale's own `\tableofcontents` would duplicate it), toptesi (`ThesisTitlePage` env + `\indici`
    prints the ToC), kks32/UCL (front matter files expected between `\maketitle` and the ToC).
  - Classes that emit the ToC or declaration inside `\maketitle`/front-matter macros (Hagenberg:
    `\maketitle` also emits the affidavit; adsphd: cover + title) — Shale's `\tableofcontents` right
    after is fine, but declaration text placement is template-controlled.
  - `\maketitle` disabled after first use (`\let\maketitle\relax` in UCL and uni-stuttgart) — harmless
    for Shale (called once).
  - Templates that redefine `\author` as a zero-arg macro (`\def\author{Your Name}` in TUM CSE) — a
    Shale `\author{value}` field would then typeset the name in the preamble or error; must not be
    mapped.
  - Two-argument `\author{first}{last}` (adsphd, yathesis) — Shale's one-arg `\author{…}` would
    swallow the next token as the second argument: silent wrong output, needs pattern 2.
  - Headings: book/KOMA-book/report templates use `\chapter`; scrartcl/article templates (maknesium,
    LMU) have no `\chapter` — Shale must map top-level headings to `\section` there; `\frontmatter`/
    `\mainmatter` exist only in book-like classes.
- **Metadata after `\begin{document}`**: tudscr example, Hagenberg, mitthesis, UCL, yathesis set
  fields in the body. Shale writes fields in the preamble; most setters are plain `\gdef`s and work
  there, but this should be checked per class (unverified).

### 3.5 Unverified items (summary)

- No template was compiled; all engine/breakage statements are from source reading.
- Overleaf gallery popularity of MastersDoctoralThesis and RWTH/TUM official templates (not on
  GitHub) were not checked.
- Logo licences: only TUDa's exclusion is explicit in the repo; for the other shipped logos the
  repo licence likely does not cover the university logo (not checked per repo).
- vutinfth: only a 3-star mirror was read; the official TU Wien distribution may be newer.
- classicthesis: only the CTAN files listed in §36 were fetched, not the full bundle.

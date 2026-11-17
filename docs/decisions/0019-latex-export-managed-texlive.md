# ADR-0019: PDF export through LaTeX templates, with a managed TeX Live

- **Status**: Accepted (owner decision, 2026-11-17)
- **Date**: 2026-11-17
- **Specs**: TEX-001…TEX-022 ([`docs/spec/tex-export.md`](../spec/tex-export.md)), PDF-001 (the built-in export stays the default)
- **Issues**: #277 (umbrella), #278 (this decision and the test stack), #279 (engine), #280 (template sets), #281 (writer), #282 (dialog, setup, user guide)

## Context

- Shale's own PDF export (PDF-001…PDF-007, pure Rust) draws a note well, but it cannot use
  corporate-design templates. The first ones wanted are the TU Darmstadt templates
  [`tuda-ci`](https://github.com/tudace/tuda_latex_templates) (v4.06): classes for thesis, PhD
  thesis, report, publication, exercise sheet, letter, beamer slides, poster, scientific poster,
  leaflet and announcement. They are LaTeX code and need a real LaTeX.
- `tuda-ci` is built and tested for **LuaLaTeX**. It needs a LaTeX kernel of 2022-06-01 or newer
  (`pdfmanagement`), the fonts Roboto and XCharter (OpenType, so `fontspec`), KOMA-Script,
  `biblatex` with `biber`, and for PDF/A `pdfx`.
- Many people do not have a TeX distribution, and one that they have may be old or lack the packages.
- The TU logo `tuda_logo.pdf` may not be redistributed: it cannot be part of Shale or of a
  template set Shale ships.

## Options

- **A. Shale manages its own minimal TeX Live.** Downloaded on first use, with consent, into
  Shale's data folder; the official `install-tl` with `scheme-infraonly`, then the packages of one
  list. A system LuaLaTeX is used when it is new enough.
- **B. Tectonic linked into the binary.** A single Rust program with its own package bundle.
  Rejected: it is XeTeX only, while `tuda-ci` is made for LuaLaTeX; its package bundle lags
  TeX Live by about a year; and it adds 30–50 MB of C libraries (HarfBuzz, ICU, FreeType, …) to the
  binary and to every build, against ADR-0006's wish for few dependencies.
- **C. Require the user's TeX.** Nothing to build, but a first-time user meets "install TeX Live
  (4+ GB) and the right packages yourself". Not acceptable as the only way.

## Decision

1. **Option A.** Shale installs and runs its own TeX Live with LuaLaTeX and `biber`, in
   `dirs::data_dir()/shale/texlive`, after the person has agreed in a dialog that shows the sizes.
   Updating and removing it is a button. Detection of a system TeX comes first (TEX-001): a
   `lualatex` on `PATH` whose kernel is 2022-06-01 or newer, with `biber`, is used as it is.
2. **The built-in pure-Rust export stays the default** (PDF-001). LaTeX is a second engine the
   person picks in the export dialog (TEX-016); nothing here makes Shale depend on TeX.
3. **Templates are not taken from TeX Live.** They are imported into the vault
   (`templates/tex/<set>/`, TEX-005, TEX-006), so a vault carries the exact class files it was
   written for, and a TeX Live update cannot change a document. TeX Live only supplies the engine
   and the generic packages.
4. **The logo is supplied by the person** (TEX-007); the tests draw a stand-in.
5. **One package list**, `assets/tex/packages.txt`, with the install profile
   `assets/tex/texlive.profile`, is used by the managed install and by the test image
   (TEX-022), so what the tests prove is what a person gets.
6. **The test stack** `tests/tex-env/` (TEX-020, TEX-021) builds an image with TeX Live from the
   frozen TL2025 repository (`tlnet-final` at `ftp.math.utah.edu`; TL2025 is frozen since
   2026-03-01), so the image is reproducible, and compiles all 25 `tuda-ci` demos at a pinned
   commit. A later TeX Live year is a deliberate change of one URL.

## Consequences

- **Disk and download** (measured 2026-11-17 with the TL2025 final repository, 134 packages:
  `scheme-infraonly` plus `assets/tex/packages.txt`, no documentation, no sources): **330 MB
  installed** (`texmf-dist` 234 MB, binaries 51 MB, generated formats and caches 24 MB) and a
  **94 MB download** (the sum of the packages' compressed containers), plus 5.5 MB for the
  installer. The whole test image, with Debian, `perl` and TeX Live, is 469 MB.
- **Time**: the install took about 2 to 3 minutes (a fast line; the packages are downloaded one
  by one). LuaLaTeX compiles `DEMO-TUDaThesis` (the biggest demo) in 13 s, including `biber` and
  the extra runs; the 25 demos together in about 1 minute with four in parallel.
- **The first LuaLaTeX run builds luaotfload's font names database** from the system fonts.
  In the container (no system fonts) it is a few seconds and 44 KB; on a machine with many fonts it
  takes longer (not measured; the managed install should do it right after installing and say so,
  rather than surprise the first export). The fonts `tuda-ci` needs (Roboto, XCharter) come from
  the TeX Live packages and need no scan.
- **The binary does not grow**: TeX Live is a separate download, not linked.
- **`perl` is needed on Linux and macOS** for `tlmgr` and `install-tl` (and for `latexmk`, which
  the tests use; Shale itself runs `lualatex` and `biber` directly). Windows' TeX Live ships its
  own. Shale tells a person without `perl` what to install.
- **The template files are in the vault**, so they are in git sync and in backups; a new template
  version is a re-import (TEX-006).
- **The CI job and `land.sh`** build the image when TeX export code or `assets/tex/` changes
  (`.forgejo/scripts/env-tests.sh`); the first build downloads the TeX Live packages and the
  tuda-ci archive.
- Pinned to TL2025 for the tests, the managed install may take a newer year later; then the test
  image is moved on purpose and the 25 demos tell whether the templates still compile.

# TU Darmstadt templates (tuda-ci)

This folder is a *template set* of Shale: the LaTeX classes of the corporate design of the
TU Darmstadt ([tuda-ci](https://github.com/tudace/tuda_latex_templates)), which Shale uses to export
notes to PDF through LaTeX. It is part of your vault, so it is synced and backed up with your notes.
Shale made it by importing the templates in **Settings → LaTeX → Template sets**.

| What | Where |
|---|---|
| The class and package files (`tudapub.cls`, `tudabeamer.cls`, …) | `tex/` |
| The document kinds (publication, report, exercise sheet, thesis, doctoral thesis, slides, letter, poster, scientific poster, leaflet, announcement) | `kinds/<kind>.toml` |
| Where the templates came from and their version | `set.toml` |
| Your choices: accent colour, language, department, logo | `settings.toml` |
| **The logo of the university** | `logo/` |

## The logo

The logo `tuda_logo.pdf` of the university may not be redistributed, so Shale cannot ship it. Get it
from the corporate design portal of the TU Darmstadt and choose it in **Settings → LaTeX → Template
sets → Choose logo…** (or copy it to `logo/tuda_logo.pdf` yourself). Until it is there, exports use a
grey placeholder bar and are not CI-conformant (they must not be used as official documents).

## Your choices (`settings.toml`)

- `accent`: the accent colour, `0a` to `9d` (the demo documents of tuda-ci use `9c`).
- `language`: `en` or `de`.
- `department`: used for the `department` field of a kind when a note does not name one.

## Updating

**Update** in the same place downloads the templates again (the pinned, tested version, or the newest
from CTAN). The old folder goes to the vault's `.trash`; `settings.toml` and `logo/` are carried over.
Changes you made by hand in `tex/` or `kinds/` are therefore in the trash after an update: copy them
back if you want to keep them.

## Changing a kind

A kind is one small TOML file in `kinds/`: the class, its options, the preamble lines and the
fields (which note property becomes which LaTeX command). The class options may use `{accent}` and
`{language}`. The user guide ("PDF") describes every key.

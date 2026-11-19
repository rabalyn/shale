# Bundled fonts

- `Virgil.otf`: the hand-drawn font by Ellinor Rapp, made for Excalidraw. It is licensed under the
  SIL Open Font License 1.1 (see `Virgil-LICENSE.md`, Reserved Font Name "Virgil").
  The file was taken from the official repository (github.com/excalidraw/virgil, `Virgil.woff2`)
  and only unpacked from WOFF2 into a plain OpenType container; no glyphs or tables were changed.
- `STIXTwoMath-Regular.otf`: the math font of the STIX Fonts Project, used to typeset math
  (EDIT-047). It is licensed under the SIL Open Font License 1.1 (see `STIXTwoMath-LICENSE.txt`,
  Reserved Font Name "TM Math"). The file is `fonts/static_otf/STIXTwoMath-Regular.otf` of release
  v2.13 b171 of the official repository (stipub/stixfonts on GitHub), unchanged.
- `LiberationSerif-Regular.ttf`: the serif font of the Liberation fonts 2.x (Red Hat, formerly
  Google's digitized data), metric-compatible with Times New Roman, so a Word document set in
  Times keeps its line breaks. It is used for a note's `print-font: serif` in the PDF export
  (PDF-014) and as the last fallback for characters no other bundled font has (PDF-010). It is
  licensed under the SIL Open Font License 1.1 (see `LiberationSerif-LICENSE.txt`, Reserved Font
  Name "Liberation"). The file is `LiberationSerif-Regular.ttf` of the `ttf-liberation` package,
  unchanged. Only the regular face is bundled: bold and italic are imitated by the PDF writer.

The colour themes that are bundled the same way are in `assets/themes/`; their authors and
licences are in `assets/themes/LICENSES.md`.

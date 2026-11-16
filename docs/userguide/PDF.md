# PDF

Back to [[Start here]].

## Export

Notes and drawings export to PDF from the note menu, with no external tools: the text stays
selectable (fonts are embedded), web links stay clickable and drawings are vector. A dialog
sets paper size, margins, fonts and more. The default path is `<vault>/exports/<name>.pdf`, so
the export shows up in the vault.

Pictures, drawings and PDFs embedded with `![[…]]` on a line of their own are drawn as
thumbnails with their link as a small caption below (the same caption as in the editor, see
[[Notes and links]]); so are the first frame of a video (needs ffmpeg) and the cover of a sound.
An embed with nothing to show keeps its link text.

The size is the one you see in the note, in proportion to the text: a picture you made half as
wide (`![[photo.png|300]]`, see [[Notes and links]]) is about as wide in the PDF in proportion to
the text, a picture that is wider than the text fills the text width, and a sound or video is
drawn as its player (cover or first frame, name, bar and length) as big as it is in the editor. The checkbox "Embedded pictures, drawings,
PDFs, videos and sounds" in the dialog switches all of this off.

Tables are exported as real tables: a bordered grid with a filled bold header row, aligned
columns and long cell text wrapping inside its column. A table that runs over a page break goes
on at the top of the next page with its header row again. Tick "Export plain text (tables as raw
text lines)" in the dialog to get the raw `| a | b |` lines instead; the box is off each time
the dialog opens, so nothing is stored in the settings.

## Read

A PDF embedded with `![[Paper.pdf]]` is drawn inside the editor and the reading view (up to 50
pages, rendered in the background). PDFs dropped on the window go through [[Import]]; see
[[Notes and links]] for embeds.

#userguide #pdf

# A template set

This folder is a *template set* of Shale for the export of notes to PDF through LaTeX. It is part of
your vault, so it is synced and backed up with your notes.

| What | Where |
|---|---|
| The class and package files (`.cls`, `.sty`, …): what LaTeX is given | `tex/` |
| The document kinds | `kinds/<kind>.toml` |
| Where the files came from | `set.toml` |
| Your choices | `settings.toml` |
| A logo, when the templates need one, and your department's logo (`department_logo.pdf` or `.png`, chosen in Settings → LaTeX) | `logo/` |

Shale knows no document kinds for these templates, so they cannot be picked for an export yet. **Add
a kind by hand**: make a file `kinds/<kind>.toml`, for example `kinds/article.toml`:

```toml
name = "Article"
mapping = "article"        # article, book, beamer, letter or poster
class = "myclass"          # a myclass.cls in tex/ (or in TeX Live)
options = ["{language}"]   # {accent} and {language} come from settings.toml
preamble = []              # lines of LaTeX after \documentclass
logo = false
# department_logo: LaTeX lines that put the department logo in, for the kinds whose class has a place for
# it; {department_logo} is the file's name. Leave it out and the kind does not show a department logo.

[[fields]]
key = "title"              # the property of the note
command = "title"          # LaTeX: \title{...}
required = true
```

The user guide ("PDF") describes every key. A re-import (**Update**) keeps `settings.toml` and
`logo/` (both logos); the old folder, with your kinds, goes to the vault's `.trash`.

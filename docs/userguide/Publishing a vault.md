---
features:
  - "A vault as a website (Hugo)"
---
Your notes can be a website: a wiki that you can read in a browser, with links between the pages, backlinks,
tags, math and a search box. Shale writes the pages; [Hugo](https://gohugo.io) builds them. Back to
[[Start here]].

This is the first step: Shale writes the site into a folder with a command. The **Settings → Publishing** page and
a button that pushes the site to your server come later, until then the rules are in a file.

# Nothing is published unless you say so

Publishing is **off** for every vault. A vault that has not turned it on writes an empty site. The switch and the
rules live in the vault's own `<vault>/.shale/settings.json` (they travel with the vault, [[Vault settings]]), in a
`publish` section:

```
{
  "version": 1,
  "publish": {
    "enabled": true,
    "include_folders": ["Blog"],
    "include_tags": ["public"],
    "exclude_tags": ["draft"]
  }
}
```

| Key | What it does |
|---|---|
| `enabled` | the switch of this vault; `false` (the default) writes no note |
| `all_notes` | every note, except the private types below |
| `include_folders` | the notes in these folders and their subfolders |
| `include_tags` | the notes with one of these tags, in the text or in the properties; `a` also takes `a/b` |
| `exclude_folders`, `exclude_tags` | notes that are never published; they win over every include rule |
| `private_types` | types that `all_notes` takes in as well; without them mail (`email`), `event`, `post` and `person` notes stay out |
| `url`, `branch` | where the site will be published later; Shale only keeps them for now |

The rules combine: a note is published when any include rule matches and no exclude rule does. A note can also
decide for itself with a property, which wins over every rule: `publish: true` or `publish: false`.

Changing a rule changes no note. Shale reads the rules and your notes and never writes to the vault.

# What is written, and what never is

- Only the chosen notes. A link from a published note to a note that is **not** published is written as its text,
  without the address, and nothing of the other note (its text, name, tags, links, backlinks) is in the site.
- A picture, sound, video, PDF or other file is written only when a published note embeds or links it.
- Never written: files and folders that start with a dot (`.shale`, `.trash`), conflict copies (`Name (conflict
  2026-11-20 1005).md`), temporary files, and mail originals (`.eml`, `.ics`).
- A media file outside the vault (`![[@Videos/clip.mp4]]` or a `file://` link, [[Sounds and videos]]) is copied when
  it is found on this computer. When it is not, the page shows the file's name instead and the command prints a
  warning.

# What a page looks like

- The page title is the note's title line (without a `YYMMDD_` date prefix); a `title:` property wins.
- `[[Note]]`, `[[Note|text]]` and `[[Note#Heading]]` become links to the pages, a heading link scrolls to the heading.
- Tags become Hugo tags with a page each. Properties like `date`, `type` or `slug` are kept for Shale (under `shale`)
  and do not change how Hugo builds the page.
- Highlights, colours, subscript and superscript, tables, footnotes, task lists (shown, not clickable), code with its
  language, and math (`$x^2$`, `$$…$$`, a `math` block) are shown; math is typeset when the site is built, so the
  browser runs no script for it. Page-break lines are left out, and a `<div align="center">` block stays aligned.
- A picture keeps its width and its alias (`![[sea.png|Sea at dusk|300]]`). Audio, video and PDF are a link to the file.
- Every folder with published notes is a page that lists its notes and subfolders; a folder note (`index.md`, `README.md`,
  or a note named like the folder) is the text of that page. The home page lists the folders and the latest notes. A
  page shows the notes that link to it in a panel below the text.

# Write the site and build it

```
shale export-site <vault folder> <output folder>
shale export-site --dry-run <vault folder> <output folder>
```

The first writes the site; the second only lists every note and media file it would write. The last line says how
many notes, media files and links were written and how many links became plain text; a problem goes to the error
output with a warning or a reason and the exit code 1. The folder is written next to the output folder first and
moved into place when it is whole, and later runs replace only the files an earlier run wrote (they are listed in
`.shale-export.json`), so a file of your own in that folder stays.

The folder is a complete Hugo site, with the Shale theme inside; nothing is downloaded when it is built. You need
[Hugo](https://gohugo.io) 0.158 or newer and, for the search, [Pagefind](https://pagefind.app):

```
cd <output folder>
hugo
npx pagefind --site public
```

The result is in `public/`: serve that folder with any web server. The pages have a search box that works once
Pagefind has run, and that finds the words of the published notes and of nothing else. The site has no login: if it must
not be public, put a password in front of it with your web server.

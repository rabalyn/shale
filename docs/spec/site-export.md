# Spec: a vault as a Hugo site (SITE)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Issue: #630.

A vault is browsable online as a wiki built with Hugo. Shale writes a Hugo site tree (content,
data, config and theme) and pushes it from the app to a publish remote, a git repository of its own
(SITE-062 to SITE-072). That is the only way to the server: the vault itself is never needed
there, and the server builds no export; it checks out the tree, runs Hugo and Pagefind and serves
the result (SITE-049). Every publish is a snapshot push, so only the published notes and their
media, off-vault media included (SITE-036), ever leave the device. Notes are chosen by publishing
rules in the vault's settings, so an old vault needs no per-note edits (SITE-051 to SITE-059). The
export is opt-in and private by default (SITE-001 to SITE-010). Phases: **1** MVP (notes, links,
heading links, picture embeds, math, colours, tags, backlinks, search, publishing rules, theme,
publishing, server recipe); **2** drawings, layouts per note type, players, the graph page; **3**
board snapshots. Every row is `SHOULD BE`; the phase of a row
is 1 unless the row says otherwise. Tests are named `file.rs::test_fn` (under `src/`).

## What is published (privacy first)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-001 | **The export is opt-in.** Only notes chosen for publishing (SITE-002) are written; a vault with no chosen note writes an empty site and says so. | SHOULD BE (#630) | — | — |
| SITE-002 | **A note is chosen for publishing** when the publishing rules of the vault choose it (SITE-051 to SITE-058); the same rules decide for every command and every note. | SHOULD BE (#630) | — | — |
| SITE-003 | **Never written:** `.shale/`, `.trash/` and every other file or folder whose name starts with `.` (VAULT-003). | SHOULD BE (#630) | — | — |
| SITE-004 | **Conflict copies are never written:** a file named `Name (conflict YYYY-MM-DD HHMM).ext` (SYNC-008) is left out, also when a published note links it. | SHOULD BE (#630) | — | — |
| SITE-005 | **Temp and sync debris is never written:** `.shale-tmp-*`, `*.part` and `*.tmp` files are left out. | SHOULD BE (#630) | — | — |
| SITE-006 | **A media file is written only when a published note embeds or links it.** A picture, sound, video or PDF that no published note names is not in the output. | SHOULD BE (#630) | — | — |
| SITE-007 | **A link to a note that is not published, or does not exist, is written as its shown text,** with no URL; the note's name is not revealed beyond the link text. | SHOULD BE (#630) | — | — |
| SITE-008 | **Mail originals (`.eml`, `.ics`) are never written,** also when a published note links them, unless the file itself is chosen for publishing (SITE-002). | SHOULD BE (#630) | — | — |
| SITE-009 | **A dry run lists what would be published:** `--dry-run` prints every note and media file that would be written, writes nothing and exits 0. | SHOULD BE (#630) | — | — |
| SITE-010 | **The output holds nothing of an unpublished note:** not its text, name, tags, links, backlinks or graph entry. | SHOULD BE (#630) | — | — |

## Publishing rules

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-051 | **Publishing is off until it is switched on** in Settings (a Publishing section); while it is off, `shale export-site` writes an empty site and says that publishing is off. | SHOULD BE (#630) | — | — |
| SITE-052 | **The rules live in the vault's settings,** `<vault>/.shale/settings.json` (CFG-005, CFG-006), which travel with the vault, so every device of the vault publishes the same notes; the publish remote's URL and branch (SITE-064) are vault settings too, but their credentials are device-local secrets as for git sync. | SHOULD BE (#630) | — | — |
| SITE-053 | **Include rules combine freely and a note is chosen when any one matches:** all notes; all notes in one or more vault folders (with their subfolders); all notes with one or more tags (front matter or inline, TAG-001). | SHOULD BE (#630) | — | — |
| SITE-054 | **A nested tag matches the rule of its parent:** a note tagged `#a/b` is chosen by a rule for the tag `#a` as well as by one for `#a/b`. | SHOULD BE (#630) | — | — |
| SITE-055 | **Exclude rules (folders, tags) win over include rules:** a note in an excluded folder or with an excluded tag is never chosen by a rule. | SHOULD BE (#630) | — | — |
| SITE-056 | **Notes of a type that holds private data are not chosen by "all notes":** `email`, `event`, `post` and `person` (IMP-003, PPL-001) are left out unless the type is ticked explicitly in the settings. | SHOULD BE (#630) | — | — |
| SITE-057 | **A note's own `publish:` property wins over every rule:** `publish: false` keeps it off the site, `publish: true` puts it on (still subject to SITE-003 to SITE-005 and SITE-008). | SHOULD BE (#630) | — | — |
| SITE-058 | **The Publishing settings show how many notes the rules choose now,** and a button lists them (the same list as `--dry-run`, SITE-009). | SHOULD BE (#630) | — | — |
| SITE-059 | **Changing a rule changes nothing in any note:** no note, front matter or file of the vault is written when a rule is added, changed or removed. | SHOULD BE (#630) | — | — |

## The command

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-011 | **`shale export-site <vault> <out>`** opens the folder `<vault>` as a vault, writes the complete site tree of SITE-070 to `<out>` without pushing it and exits without opening a window, like `--export-pdf` (BOOK-011); it is for tests and for people with their own pipeline. | SHOULD BE (#630) | — | — |
| SITE-012 | **The command line takes exactly a vault folder and an output folder** (and `--dry-run`); anything else, or a vault folder that does not exist, exits 1 with the reason on stderr and writes nothing. | SHOULD BE (#630) | — | — |
| SITE-013 | **A failed run leaves `<out>` as it was:** the new tree is written next to `<out>` and moved in only when it is complete (SITE-014 says which files it replaces). | SHOULD BE (#630) | — | — |
| SITE-014 | **Only Shale's own files are replaced:** a file of `<out>` that an earlier export did not write (listed in `<out>/.shale-export.json`) is kept. | SHOULD BE (#630) | — | — |
| SITE-015 | **Exit code and summary:** 0 on success, 1 with the reason on stderr; the last line on stdout says how many notes, media files and links were written and how many links became plain text. | SHOULD BE (#630) | — | — |
| SITE-016 | **The export reads the vault and never changes it:** no note, no `.shale/` file and no timestamp of the vault changes. | SHOULD BE (#630) | — | — |

## Links and headings

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-017 | **Wiki links resolve as in the editor:** `[[T]]`, `[[T\|alias]]`, `[[T#H]]` and `[[#H]]` find their target by the vault's rules (LINK-002, LINK-003, LINK-010); an ambiguous or missing target counts as not published (SITE-007). | SHOULD BE (#630) | — | — |
| SITE-018 | **Markdown links to notes resolve the same way:** `[x](Note.md)` and `[x](Folder/Note.md#H)` (LINK-006) find their note and are written as links to its page; web links stay as they are. | SHOULD BE (#630) | — | — |
| SITE-019 | **A resolved link is written as a standard Markdown link** to the target page's URL, with the alias or the note's title as its text. | SHOULD BE (#630) | — | — |
| SITE-020 | **Heading links land:** a link to `#H` points to the anchor Hugo makes for that heading by its default rule, so the browser scrolls to it. | SHOULD BE (#630) | — | — |
| SITE-021 | **The page title is the title line without its `YYMMDD_` date prefix** (VAULT-025, CFG-043); the file keeps its name. | SHOULD BE (#630) | — | — |
| SITE-022 | **URLs are stable and lower case:** a page's URL is its vault path made into a slug; the same path gives the same URL on every export, and two paths never give one URL. | SHOULD BE (#630) | — | — |

## Front matter

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-023 | **Tags become Hugo `tags`:** the tags of a note's front matter and its inline `#tag`s (TAG-001) are written once each as the page's `tags`. | SHOULD BE (#630) | — | — |
| SITE-024 | **Keys Hugo reserves are moved when they mean something in Shale:** `date`, `draft`, `type`, `layout`, `url`, `slug`, `weight` and `aliases` of a note are written under a namespaced param (`shale.date`, …), so Hugo does not act on them. | SHOULD BE (#630) | — | — |
| SITE-025 | **Shale's `type:` goes to a param the theme reads** (`shale.type`), which picks the layout of SITE-044. | SHOULD BE (#630) | — | — |
| SITE-026 | **The page's `title` is the title of SITE-021;** a `title:` in the front matter wins. | SHOULD BE (#630) | — | — |

## Formatting

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-027 | **Marks and spans survive:** `==x==`, `<mark style>`, `<span style>`, `<u>`, `<sub>` and `<sup>` (EDIT-028, EDIT-051) show on the page with their colours. | SHOULD BE (#630) | — | — |
| SITE-028 | **Math is typeset when the site is built:** `$…$`, `$$…$$` and a ```` ```math ```` block (EDIT-046) show as math without a script in the browser. | SHOULD BE (#630) | — | — |
| SITE-029 | **Footnotes, tables and task lists** show as in the editor; a task's box is shown checked or not and cannot be changed. | SHOULD BE (#630) | — | — |
| SITE-030 | **Code blocks keep their language** and are highlighted by it. | SHOULD BE (#630) | — | — |
| SITE-031 | **Page-break divs are dropped** (PDF-020): the page has no trace of them. | SHOULD BE (#630) | — | — |
| SITE-032 | **`<div align>` is kept** (PDF-028): the block is aligned as in the note. | SHOULD BE (#630) | — | — |

## Embeds and media

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-033 | **A picture embed shows its picture:** `![[x.png]]` and `![](x.png)` of a published note write the file (SITE-006) and show it. | SHOULD BE (#630) | — | — |
| SITE-034 | **A picture embed keeps its width and alias:** `![[x.png\|300]]` shows the picture 300 pixels wide (EDIT-052), and its alias is the alt text (EDIT-053). | SHOULD BE (#630) | — | — |
| SITE-035 | **Audio, video and PDF embeds show a player** (phase 2); in phase 1 they show a link to the file. | SHOULD BE (#630) | — | — |
| SITE-036 | **Off-vault media are copied in when they resolve on this device:** a library link `@Name/…` or a `file://` link (ADR-0030, MEDIA-014, MEDIA-015, MEDIA-018) that a published note embeds or links and that resolves here (the device-local `media_libraries` setting) is copied into the tree under a stable path and published like vault media (SITE-006, SITE-069); one that does not resolve here writes no file, shows a placeholder that names the file, and is listed as a warning in the publish summary (SITE-067). | SHOULD BE (#630) | — | — |

## Drawings

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-037 | **A drawing is written as an SVG picture** (phase 2): `.excalidraw`, `.excalidraw.md` and `.canvas` files that a published note embeds are drawn to SVG and shown as a picture. | SHOULD BE (#630) | — | — |
| SITE-038 | **`.excalidraw.md` is never a note:** it gets no page of its own, no backlinks and no entry in the graph (DRAW-007, DRAW-008). | SHOULD BE (#630) | — | — |

## Wiki pages

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-039 | **Backlinks per page:** `data/backlinks.json` lists, for every published note, the published notes that link it, with their titles and URLs. | SHOULD BE (#630) | — | — |
| SITE-040 | **The graph data holds published notes only:** `data/graph.json` has the nodes and links of LINK-011 minus every unpublished note and every link to or from one (phase 2 for the graph page). | SHOULD BE (#630) | — | — |
| SITE-041 | **Folders become sections:** a folder with published notes is a section whose generated index lists its notes and subfolders; a folder note (BOOK-002) is the section's text. | SHOULD BE (#630) | — | — |
| SITE-042 | **A page shows its backlinks** in a panel below the text; a page with none shows no panel. | SHOULD BE (#630) | — | — |
| SITE-043 | **The site has an index page** that lists the top-level sections and the most recently changed notes. | SHOULD BE (#630) | — | — |
| SITE-044 | **A layout per `type:`** (phase 2): notes of type email, event, post and person show their own layout; any other type shows the plain page. | SHOULD BE (#630) | — | — |
| SITE-045 | **A board is a fixed snapshot** (phase 3): a published board note (BOARD) shows its lanes and cards as they were at export time, without drag or edit. | SHOULD BE (#630) | — | — |

## Theme and server

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-046 | **Shale ships a Hugo theme** (a Hugo module) with the render hooks, layouts and config the export needs, so a site builds with it and no further setup. | SHOULD BE (#630) | — | — |
| SITE-047 | **The theme's config sets Goldmark right:** the `extras` of Goldmark have subscript off (`~~` is strike-through), unsafe HTML is on, and math passes through to the build. | SHOULD BE (#630) | — | — |
| SITE-048 | **Full-text search with Pagefind:** the site has a search box that finds words of published notes, and of nothing else. | SHOULD BE (#630) | — | — |
| SITE-049 | **A server recipe is documented:** on a push to the publish branch (a Forgejo Actions job or a post-receive hook) the server checks out with git-lfs, runs `hugo` and `pagefind` and swaps the output folder in; no Shale runs on the server. | SHOULD BE (#630) | — | — |
| SITE-050 | **Access control is the web server's** (basic auth or an OIDC proxy): Shale writes no login; the recipe says so and says that the site is public otherwise. | SHOULD BE (#630) | — | — |

## Publishing to the server

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SITE-060 | *Retired* (replaced by SITE-062): the server was to build from the vault synced by git; the vault is never needed on the wiki server. | Retired | — | — |
| SITE-061 | *Retired* (replaced by SITE-062): the rules were to be applied on the server; the app applies them before it pushes. | Retired | — | — |
| SITE-062 | **The only way to the server is a push from the app (phase 1):** a publish runs the export of the chosen notes (SITE-002) and pushes the generated tree to the publish remote; the server never receives the vault and builds no export. | SHOULD BE (#630) | — | — |
| SITE-063 | **The publish repository is a git repository of its own** in Shale's device-local data folder, not inside the vault and not the vault's repository: the vault's git sync and its `origin` never receive it (SYNC-004), and the publish remote never receives the vault. | SHOULD BE (#630) | — | — |
| SITE-064 | **Settings → Publishing hold the publish remote's URL and branch;** the URL is accepted by the rules of SYNC-004 and refused with a password in it (SEC-010), credentials and SSH host keys are handled as for git sync (SYNC-052 to SYNC-056), and git is the managed or system git of SYNC-024 (ADR-0027). | SHOULD BE (#630) | — | — |
| SITE-065 | **A Publish action runs export and push:** a command and a button in the Publishing settings start it as a task that shows its progress and can be cancelled (JOB-003 to JOB-006); a cancelled or failed publish leaves the remote as it was. | SHOULD BE (#630) | — | — |
| SITE-066 | **`shale export-site --publish <vault>` does the same without a window** (like BOOK-011): it publishes the vault's chosen notes to the publish remote of its settings, prints the summary of SITE-015 and exits 0, or exits 1 with the reason on stderr. | SHOULD BE (#630) | — | — |
| SITE-067 | **The publish summary** says how many notes and media files were published, how many links became plain text, and lists every warning, among them each off-vault file that did not resolve on this device (SITE-036). | SHOULD BE (#630) | — | — |
| SITE-068 | **Each publish is one snapshot commit with no parent,** force-pushed to the publish branch, so the remote holds only the current published state: a note that is no longer published is gone from the branch and its history after the next publish. | SHOULD BE (#630) | — | — |
| SITE-069 | **Media go through git LFS with the patterns of the vault** (SYNC-017, SYNC-021): only files that a published note embeds or links are in the tree (SITE-006), and a media file already on the remote's LFS store is not uploaded again. | SHOULD BE (#630) | — | — |
| SITE-070 | **The tree is a complete Hugo site:** content, data files, the Hugo config and the Shale theme (SITE-046) vendored in, so the server needs only `hugo` and `pagefind` (no Shale, no download of Hugo modules). | SHOULD BE (#630) | — | — |
| SITE-071 | **LFS objects of media that are no longer published stay on the LFS server** until it collects them; the Publishing settings and the user guide say so. | SHOULD BE (#630) | — | — |
| SITE-072 | **The last publish wins:** two devices may publish to one remote without coordination (SITE-068); the Publishing settings show from which device and when the remote was last published, read from the commit's message, which names the device and the time like SYNC-036. | SHOULD BE (#630) | — | — |

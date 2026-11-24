---
features:
  - "A vault as a website (Hugo)"
---
Your notes can be a website: a wiki that you can read in a browser, with links between the pages, backlinks,
tags, math and a search box. Shale writes the pages; [Hugo](https://gohugo.io) builds them. Back to
[[Start here]].

Shale can write the site into a folder (a command), and it can **publish** it: write it and push it to a
repository on your own server, where a small job builds it with Hugo and Pagefind and serves it
([[Publishing to a server]]). The vault itself never goes to the server, only the notes you chose and the pictures
and files they show.

# Nothing is published unless you say so

Publishing is **off** for every vault. A vault that has not turned it on, or that has no chosen note, writes a site with one **hello-world page** (it says it was published with Shale and that no notes are chosen yet, with the time, how to publish notes and a link to this note, and nothing of your vault), so you can see that publishing and your server's build work. You turn it on, choose
the notes and say where to publish in **Settings → Publishing** (below). All of it is the vault's own setting in
`<vault>/.shale/settings.json` (it travels with the vault, [[Vault settings]]), in a `publish` section, which you can
also edit by hand:

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
| `url`, `branch` | the repository the site is pushed to and its branch (`main` when empty) |

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
- A picture keeps its width and its alias (`![[sea.png|Sea at dusk|300]]`). An embedded sound, video or PDF
  (`![[song.mp3]]`, `![[clip.mp4|480]]`, `![[paper.pdf]]`) is shown with a player in the page, the PDF in a frame, and
  a link to the file inside for a browser that cannot show it; the width works for them too. A plain link
  (`[[song.mp3]]`) stays a link.
- A drawing (`.excalidraw`, `.excalidraw.md` or an Obsidian `.canvas`) that a note **embeds** (`![[Map.excalidraw]]`)
  is drawn to a picture (SVG) in the site, with the width and alias of a picture. A link in the drawing to a note that
  is published works; a link to a note that is not is only its text. A drawing that no published note embeds is not
  written, and a drawing is never a page of its own.
- Notes whose `type` is `email`, `event`, `post` (a saved post) or `person` show a header with their properties (an
  e-mail: from, to, date; an event: when, where, who; a post: author, date, source; a contact: e-mail, phone,
  organization), as they are written in the note and nothing else. Notes of any other type look like every page.
- A **board** (a note with `type: board`) that is chosen for publishing shows its lanes as columns, side by side (they
  scroll sideways), with the cards as they were when you published: the lane names, the cards in the order the board
  shows them, and the chips the board shows (due day, status, priority, tags). It is a fixed picture: nothing can be
  dragged or edited. A card of a published note links to that note's page. **A card of a note that is not published is
  left out altogether**, with no title, no count and no gap, so the page gives no hint that it exists; the lanes'
  queries and pinned links are not written either. The page's own text is what stands above the first lane. A lane that
  cannot be worked out (a query with an error) shows a short note instead of cards, and the publish summary lists it as
  a warning.
- The site has a **Graph** page (`graph.html`, linked in the header of every page) with the published notes as dots and
  the links between them as lines: zoom with the wheel, drag a dot or the background, click a dot to open the note. It
  needs no server and loads nothing from another site.
- The site has a **Site map** page (`sitemap.html`, linked in the header next to the graph): every published note as a
  tree by folder, a folder linking to its folder note. It is plain HTML, works without an address of the site and lists
  only the notes you publish.
- With an **address of the site** the site also has a **`sitemap.xml`** with the full address of the home page, every
  folder page and every note page, and nothing else: no dates, no tag pages and not the graph or the site map page. The
  `robots.txt` next to it allows search engines and names the sitemap. Publishing is no more or less public with
  them: anyone who can reach the site can read it, see [[Publishing to a server]]. When the address has a path
  (`https://host.example/wiki/`) every link of the site goes below it, so serve the folder at that path.
- Every folder with published notes is a page that lists its notes and subfolders; a folder note (`index.md`, `README.md`,
  or a note named like the folder) is the text of that page. The home page lists the folders and the latest notes. A
  page shows the notes that link to it in a panel below the text.

# Publish from Shale

Open **Settings → Publishing**. With the switch off the page shows only the switch. With it on:

![[settings-publishing.png]]

- **Which notes**: all notes, the notes in folders, the notes with tags, and what is never published (one entry per
  line); the e-mails, events, posts and contacts that "all notes" leaves out can be ticked in. The page says how
  many notes the rules choose now, and **List the notes** shows them, the same list as `--dry-run` below.
- **Where to publish**: the address of a repository of its own, and its branch. Use a repository that is only for
  the site, never the one this vault syncs with: Shale refuses that address, because a publish replaces everything on
  the branch. A password in the address is refused too; sign in as for git sync ([[Turn on sync]]): a **token** for an
  `https://` address (Shale keeps one token for each server and user name, so two vaults on the same server use the
  same sign-in) or an **SSH key**. When the sign-in does not work, the page says why in the words of
  [[Git sync sign-in problems]] with a link **How to fix this**.
- **Address of the site** (optional): the public address where the built site is served, for example
  `https://wiki.example.org/` or `https://host.example/wiki/`. Only `http://` and `https://` addresses are accepted,
  without a user name or password; the page tells why when one is refused and does not keep it. With an address the
  site gets a `sitemap.xml` and a `robots.txt` for search engines (below); without one it has neither and its links
  are paths, which work wherever you put the folder.
- **Publish** writes the site and pushes it, in the background, with the step it is at and **Cancel**. When it is
  done the page says how many notes and media files were published and how many links became plain text, and lists
  every warning, for example each file outside the vault that was not found on this computer. A publish that is
  cancelled or fails leaves the repository as it was. The same is `shale export-site --publish <vault folder>` on
  the command line (it prints the same summary; see below). Only one publish of a vault runs at a time: a second
  one (the button, the command line or another Shale) stops (after at most half a second) with "Another publish of this vault is running"
  and changes nothing.
- **Who published last**: the page reads the branch's last commit and says from which computer and when it was
  published. **Check the target** asks again.

Every publish is **one snapshot**: a single commit with no parent that replaces the branch. A note that you stop
publishing is therefore gone from the branch and from its history at the next publish, and the repository stays small.
Two computers can publish to the same repository without telling each other: the last publish wins. The commit's
message names the computer, the time, the vault and its name.

**Another vault's target.** Before it pushes, Shale reads the branch's last commit. When another vault published
there, or the branch was not made by Shale, a question says so and asks whether this vault takes the target over;
nothing is pushed until you answer. A branch that does not exist yet is taken without asking. On the command line the
answer is `--take-over`.

**Pictures, sound and video.** The audio and video files go to the server's **Git LFS** store, the way git sync does it
([[Git sync]]), so Git LFS has to be installed on this computer; a file that is already in the server's store is not
sent again. A file that you stop publishing stays in the store until the server collects what nothing uses any more.
Pictures and PDFs are ordinary files of the snapshot.

Shale keeps the repository it pushes from in its own data folder, one for each vault (`<data folder>/shale/vaults/<vault
id>/publish`), never in the vault; the vault gets one small file, `.shale/history-id`, its ID, if it has none yet.

# Write the site and build it

```
shale export-site <vault folder> <output folder>
shale export-site --dry-run <vault folder> <output folder>
shale export-site --publish <vault folder> [--take-over]
```

The first writes the site; the second only lists every note and media file it would write. The last line says how
many notes, media files and links were written and how many links became plain text; a problem goes to the error
output with a warning or a reason and the exit code 1. The third form publishes the vault, with the settings of
the vault (switch, rules, address and branch) and the sign-in of this computer; it names one vault, never all of them. The folder is written next to the output folder first and
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
not be public, put a password in front of it with your web server. How a server builds the site by itself whenever
you publish is in [[Publishing to a server]].

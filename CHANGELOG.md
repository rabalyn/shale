# Changelog

All notable changes to Shale are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (before 1.0, new features raise the middle number and
fixes the last one).

The release page of each version on GitHub shows its section from this file.

## [Unreleased]

### Added

- **Drag and drop in the file list**: drag notes, drawings, files or whole folders onto a folder
  to move them there (onto a note: next to it; onto empty space: to the top of the vault). A
  closed folder opens when you hold the pointer on it, the list scrolls at its edges, and links
  written as a path to anything that moved are updated (links by name keep working anyway).
- **Browse search results with the keyboard**: ↓ or Enter in the search box goes into the
  results; j / k move from hit to hit, Ctrl+J / Ctrl+K jump to the next / previous file, and
  Enter opens the note with the cursor on the found text, ready to type. Esc goes back to the
  box. All hits of a note are listed now (up to 20), not only the first 6.
- **Obsidian Excalidraw drawings** (`.excalidraw.md`) open in the drawing editor instead of as a
  note, also when their data is compressed. Saving keeps the plugin's file layout (front
  matter, compression, embedded files) and updates its text list, so Obsidian keeps reading the
  drawing. `[[Drawing.excalidraw]]` links and embeds find these files.
- **Keyboard-only navigation**: Ctrl+H puts the keyboard into the left sidebar, Ctrl+L back
  into the note, and Ctrl+Shift+B opens the file list. There j / k go from file to file,
  Ctrl+J / Ctrl+K from folder to folder, Space opens or closes a folder and Enter opens a file.
- **Vim-style editing** (Settings, off by default): notes open in normal mode with h j k l,
  w b W B, 0 $, gg G, `:line`, `/search` with n / N, i a I A o O to type, x, visual mode (v, V)
  with d to cut and y to copy to the clipboard, and p to paste.
- **Git sync**: connect a vault to a git repository in Settings (or `git init` it yourself) and
  Shale commits, merges and pushes on its own, every few minutes and on "Sync now". Conflicts
  become conflict copies next to the note; nothing is force-pushed or overwritten. The interval
  and a commit message prefix / suffix are settings.
- **Vault settings live in the vault**: `.shale/settings.json` (people folder, git sync) and
  `.shale/state.json` (open notes, last import folder, recent tags), versioned and upgraded on
  load, so a synced vault behaves the same everywhere. They move there from the global settings
  on the first start.

### Fixed

- **Data safety (ADR-0003)**: a note changed by another program while you edit it is saved as
  `Name (conflict date time).md` next to it and the other version stays, instead of being
  overwritten; open drawings reload when changed outside; a note that cannot be read (not UTF-8,
  no permission) opens read-only and is never written; failed saves stay visible (red dot on the
  tab, "Not saved" in the status bar) and keep your text; a rename or move that cannot update
  every link says which notes were and were not updated. A randomized test runs thousands of
  such operations and checks that nothing is lost.
- A settings file that can't be read is no longer replaced by defaults on the next save (which
  lost the calendars): it is kept as `config.json.broken-<date>` and you are told. Settings from
  a newer Shale are no longer overwritten.

- Opening a drawing with arrows no longer rewrites its file; only an edit does.
- A drawing file that can't be read now opens read-only with the reason, instead of as an empty
  drawing that would replace the file on the first edit.
- Renaming a note rewrites only the links that really pointed to it, keeps their folder part
  (`[[Projects/Old]]` → `[[Projects/New]]`), their `#heading` in markdown links, `<…>` and
  `.md`, and compares names with Unicode case. Renaming a **folder** now updates links to the
  notes inside it, and moving a note keeps its own `../` links working.
- A rename can no longer move a note out of the vault (`../../x`, `/tmp/x`), and a new folder
  name can't point outside it either.

### Changed

- **Path links are stricter**: `[[Archive/Plan]]` is now red when `Archive/Plan.md` does not
  exist, instead of quietly opening some other note called “Plan”. `./` and `../` links work
  relative to the note. If exactly one note has that name, the link is fixed automatically
  when you open the note; if several do, the Outgoing links panel offers “Fix link”.
- The tooltip of a link says when several notes share its name and which one it opens.
- Dragging something Shale can't take (like a browser tab) across the window no longer leaves a
  message behind; a hint at the pointer says so while it is over the window.

## [0.3.0] - 2026-11-13

### Security
- CalDAV passwords are kept in the system keyring (Secret Service via `secret-tool`) instead of
  the settings file; without a keyring they stay in the owner-only settings file as before.
- Only `https://` CalDAV addresses are accepted (`http://` only for a server on this computer);
  addresses with a user name or password inside are refused.
- The password is never sent to another server: event addresses from the server must be on the
  calendar's own server, and redirects may only go to https.
- `~/.curlrc` is no longer read for CalDAV requests, version tags with line breaks are refused
  (no header injection), and server answers are limited to 50 MB.
- The keyboard recording (troubleshooting) no longer records anything typed in the calendar
  settings, and its file is readable only by its owner.
- The settings file is written owner-only from the start and replaced in one step (no window
  where it is readable by others, no half-written file after a crash).

### Fixed
- CalDAV: a real XML parser reads the server's answers (namespaces, CDATA, comments); document
  type declarations are refused.
- CalDAV discovery follows the address the server redirected to, leaves out task-only lists and
  marks read-only calendars; read-only calendars can't be chosen for saving.
- CalDAV: the new version tag of a saved event is kept, an event without a known version tag is
  checked before it is overwritten, and different event IDs never share a file on the server.
- CalDAV calendars that didn't change (same ctag / sync-token) aren't downloaded again; "Sync now"
  always downloads.

## [0.2.0] - 2026-11-13

### Added

- **People (contact notes)**: a people folder (default `Adressbuch`, set in Settings → People).
  Type `[[p ` and a name to pick a contact from that folder; the link written is a normal
  `[[Name]]` link. "New contact" makes a contact note from a template (e-mail, phone,
  organization, notes).
- **People when importing**: sender, recipients, organizer and attendees of imported e-mails and
  events are matched to contact notes (by e-mail address, else by name; "Müller, Hans" is
  "Hans Müller"). Known contacts and new senders are ticked; ticked people without a note get
  one, and the imported note gets a "People" line with links. Automatic senders ("noreply") are
  left out.
- **Automatic tags when importing**: tags of the vault written in the imported text are set right
  away (`#lecture-hall-b` for "Lecture Hall B"). Shale also learns from the notes which words are
  typical for each tag and suggests those tags — plain counting, deterministic, no server. The
  word lists are relearned in the background and kept in `.shale/tag-words.json` in the vault.
- The version is shown in the status bar.

### Fixed

- Text with umlauts and other non-ASCII characters can no longer crash the app: due dates like
  `due:Müllabfuhr` and the time field of the event dialog cut characters in half. A new test runs
  every text function with umlauts, ß, €, emoji and combining marks at every position.
- Editing an event no longer replaces the organizer's and attendees' e-mail addresses with
  placeholders (CalDAV servers kept the wrong addresses).

## [0.1.0] - 2026-11-13

First public version (published before releases were tagged).

### Added

- Markdown vault editor with `[[wiki links]]` (blue when the note exists, red when not; click to
  open or create), link suggestions, a file picker (`[!`), lists that continue, indenting,
  moving lines, inline pictures, drawings and PDFs, reading view, PDF export.
- Drawings stored as `.excalidraw` files with a style panel, labels, snapping arrows and automatic
  layout.
- To-do list across all notes, backlinks, outline, local graph and vault graph, tags, search.
- Calendar with month/week/day views, local and CalDAV calendars (sync interval per calendar),
  creating, editing and moving events safely between calendars.
- Import of e-mails, events and files by drag and drop (also from Thunderbird on Wayland, with a
  lookup in Thunderbird's local copy) with folder, tags and attachments.
- Ctrl+Shift+F always focuses the search box and selects the previous search.
- File names with `#` work in links.

[Unreleased]: https://github.com/rabalyn/shale/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/rabalyn/shale/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rabalyn/shale/compare/6851a04...v0.2.0
[0.1.0]: https://github.com/rabalyn/shale/tree/6851a043e3c1f98af9b92575a43a93202b3b13b7

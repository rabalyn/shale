# Changelog

All notable changes to Shale are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (before 1.0, new features raise the middle number and
fixes the last one).

The release page of each version on GitHub shows its section from this file.

## [Unreleased]

### Added

- **Mail view**: read your IMAP mail in Shale. Ribbon button or Ctrl+Shift+M opens
  accounts and folders (with unread counts), a fast message list (50,000 messages are fine) and
  a plain-text reading pane with the attachments. Accounts are set up in "Accounts…" (password
  in the system keyring, or asked per session without one) with a "Test
  connection" button and a tick box per folder. Mail is synced in the background (on a button and
  every 10 minutes), Trash, Junk and Gmail's All Mail are listed only when you open them, and a
  message is downloaded with "Load this message". Search understands `from:`, `to:`, `subject:`
  and "phrases". "Import to vault" (or `i`) hands a mail, or all marked mails, to the Import
  dialog like a dragged `.eml`. j / k, Enter, Space, Ctrl+J / Ctrl+K, `/` and Esc work without
  the mouse. Syncing never changes anything on the server, shows no HTML and loads nothing from the
  web. **The local mail copy is not encrypted: use full-disk encryption.**
- **Mail: write and send** (phase 3, SMTP). `c` compose, `r` reply, `Shift+R` reply all, `Shift+F`
  forward (`f` is still the flag key), Ctrl+Enter send, Esc close. Plain text with attachments (a
  file, a file of the vault, or a note as markdown), addresses completed from your people notes and
  your mail, a signature, correct `In-Reply-To` / `References`, and the original quoted. Sending is
  set up per account (TLS 465 or mandatory STARTTLS 587, the same password or OAuth2 sign-in as for
  reading; Google and Microsoft are presets). Drafts are saved by themselves (atomically; after a
  crash they are in Drafts), closing asks keep or discard (discard goes to `.trash/`), Send puts the
  mail in an Outbox with an **Undo send** window (10 seconds, setting), and a mail is never sent twice:
  a failure keeps it in the Outbox until you retry, and a mail that may or may not have gone out (the
  connection ended after the whole mail was handed over, or Shale was closed while sending) is shown
  as "may have been sent" for you to decide. Confirmations for an empty subject or text and a
  forgotten attachment. Bcc never appears in the mail, no read receipt, `User-Agent` only on request.
  After sending, the exact message appears in Sent at once and is saved to the server with `APPEND`
  through the same write chokepoint as phase 2 (a failed save never causes a resend). New: ADR-0010,
  `src/compose.rs`, `src/smtp_client.rs` (own small SMTP client; `lettre` only builds the message),
  `src/outbox.rs`, `src/mail_send.rs`, a randomized safety test, and the container test environment
  now has Dovecot's submission service with a Mailpit sink.
- **Mail: signed and encrypted mail** (phase 4, OpenPGP and S/MIME with the system's GnuPG: `gpg`,
  `gpgsm`, `gpg-agent`; no key or passphrase ever passes through Shale, no key lookup on the network).
  A status strip above the mail shows what GnuPG found (valid with signer, fingerprint and trust; bad;
  unknown, expired or revoked key; partly signed; not the sender's address; inline OpenPGP as weaker), built
  from GnuPG's status lines only. Encrypted mail is decrypted when opened, in a job (a passphrase dialog
  may open, Cancel stops it), but **only after the consent in Accounts…, Encryption**: a red banner
  ("encrypted mails WILL ALWAYS BE STORED DECRYPTED LOCALLY") and the exact sentence
  `I agree to decrypted local storage`. The decrypted copy is stored beside the untouched original and
  indexed for search; withdrawing the consent stops decrypting and deletes nothing. EFAIL-safe: only
  wholly encrypted mail is decrypted, decrypted text is its own message, nothing is joined. Writing: Sign
  and Encrypt in the compose window (key found / no key per recipient from the local keyrings, Bcc
  included, always also encrypted to yourself), account key pickers, PGP/MIME (RFC 3156) and S/MIME (RFC 8551)
  built in the sending pipeline with the signed bytes exactly the bytes sent. Signing runs in a job and a
  failure keeps the mail in the window. The subject of an encrypted mail is not encrypted. New: ADR-0011,
  `src/gpg.rs`, `src/mail_crypto.rs`; tests with real keys in a temp `GNUPGHOME` and one against the
  container environment. Your Thunderbird keys: `gpg --import` (README and the settings explain).
- **Fixed**: the local copy of a sent mail, which waits for the server's own, is kept under UIDVALIDITY 0 with
  its own file, so a real server UID can never overwrite or merge with it; the next sync replaces it by the
  server's copy (same Message-ID) and keeps the local file in `.trash/`.
- **Mail: read, flag, move and delete** (phase 2, two-way). `u` read / unread, `s` or `f` flag,
  `d` or Delete to Trash, `m` move (a folder picker you type into), on the open mail, the one
  under the cursor or all marked mails; also as buttons, in the right-click menu of a row and in
  the marked bar. Unread mail is bold, flagged mail has a flag, and changes the server has not
  confirmed yet say so. Changes show at once, wait while you are offline (also across a restart)
  and are undone with a message if the server refuses. "Delete" always moves to the Trash folder;
  "Delete forever" exists only in Trash, asks first and removes only the mails you chose. Mail
  that another program moved or deleted meanwhile is left alone. Opening a mail can mark it read
  after 2 seconds (setting, on by default, in Accounts…). Syncing still never writes to the
  server, and Shale never deletes its own local copy (old files go to `.trash/` in the mail
  folder). New: ADR-0009, `src/mail_ops.rs`, a randomized safety test that checks no mail is ever
  lost, and tests against the Dovecot container.
- **OAuth2 sign-in for mail** (Google, Microsoft, or a custom provider): authorization code with
  PKCE in your browser, a refresh token in the system keyring, `XOAUTH2` or `OAUTHBEARER` at the
  server, and a clear "sign in again" when the sign-in expires or is revoked. Shale has no client
  IDs of its own yet: you enter one you registered (README explains how, in five lines per
  provider). Google's restricted mail scope limits an unverified app to 100 test users.
- **A local mail test environment** (`tests/mail-env/`, podman-compose): Dovecot with TLS and
  seeded mailboxes plus a pretend OAuth2 provider, and tests that sync both accounts against it
  and check that nothing on the server changed (`cargo test -- --ignored mail_env`).
- **Unsaved text is no longer lost at exit**: if a note still cannot be saved when Shale quits,
  its text is written to `~/.local/share/shale/recovery/<date>/…` (never over anything) and the
  next start tells you, with a link to the folder.
- `--screenshot … --view mail` shows the mail view with a made-up mail store.
- **Feeds: Mastodon and Bluesky**: a new Feeds tab (speech-bubble button in the ribbon) to
  scroll your Home timeline, lists, Bluesky Following and saved feeds, with avatars, pictures,
  quotes, link cards, content warnings and "boosted by". The next page loads when you reach the
  end. **Save to vault** opens the Import dialog with the post, or the whole thread when it is a
  thread by one author, its pictures (each can be unticked) and a **Source** link back; posts
  saved before are marked. An option adds other people's replies as a nested "Replies" section. Accounts are added in the dialog **Accounts…**: Mastodon with a code
  from the server's page (or an access token), Bluesky with an app password; secrets go to the
  system keyring. `--view social` and `--view social-import` (with `--screenshot`) show it with
  made-up posts. The `--keys` script got a `scroll:N` step.
- **Several feeds in tabs, and new posts that wait**: each Feeds tab keeps its own feed, scroll
  position and history ("New tab" next to each feed in the picker, Shift+click or middle-click),
  titled "Mastodon · Home" with the number of waiting posts, and open Feeds tabs are restored at
  the next start. Every 5 minutes (setting in **Accounts…**, 0 = off) the feeds open in tabs look
  for newer posts and show "N new posts — show" in a row above the list (which says "Up to date ·
  checked 2 min ago" otherwise, so the list never moves and no post is covered) instead of inserting
  them; clicking it (or `.`) puts them above while the post you were reading stays at the same
  place on the screen. **Refresh** is now **Check now** (it no longer jumps to the top);
  **⋯ → Reload from the top** replaces the list.
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
- **Obsidian canvases** (`.canvas`) open in the drawing view, read-only: cards with their
  colours, groups, file and web cards, and labelled arrows. Opening one asks whether to convert
  it into an editable `Name.excalidraw.md` drawing next to it (the canvas is never changed); a
  tick box, also in Settings → Drawings, makes that automatic. A copy made earlier is offered
  instead of a second one.
- **Links in drawings**: `[[` note suggestions, `[[p ` people and the `[!` file picker work in
  drawing labels and texts. Shapes and texts with a link show a ↗ mark; click it (or Ctrl+click
  the shape) to open the note or web page.
- **Drawings in backlinks and the graph**: links in drawings and canvases count like links in
  notes. Renaming or moving a note updates the links in drawings as well; canvases are never
  changed, and the message names those still pointing to the old name.
- **Pictures in drawings**: pictures in Obsidian Excalidraw drawings and excalidraw.com files are
  shown instead of a dashed box, also in embeds and PDF export.

### Fixed

- A note or drawing changed by another program right after Shale read it (inside one timestamp tick of the file system) was not reloaded. Shale now also compares size and file identity, not only the modification time.
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

- **Every dialog can be moved and resized**: drag the title bar to move it, any edge or corner
  to resize it; the content scrolls when it is made small. Dialogs open centred and as tall as
  their content, and come back where you left them. The headings that were inside the dialogs
  are now their title bars, with a ✕ to close.
- **Path links are stricter**: `[[Archive/Plan]]` is now red when `Archive/Plan.md` does not
  exist, instead of quietly opening some other note called “Plan”. `./` and `../` links work
  relative to the note. If exactly one note has that name, the link is fixed automatically
  when you open the note; if several do, the Outgoing links panel offers “Fix link”.
- The tooltip of a link says when several notes share its name and which one it opens.
- The `[[`, `[[p ` and `[!` helpers moved out of the note editor into their own module
  (`src/link_assist.rs`) so drawings can use them; notes behave as before.
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

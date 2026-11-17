# Changelog

All notable changes to Shale are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (before 1.0, new features raise the middle number and
fixes the last one).

The release page of each version on GitHub shows the AI notice (`.github/ai-notice.md`) and then
its section from this file.

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

- **Calendar: repeats, single dates and reminders can be edited.** The event dialog sets how an
  event repeats (daily, weekly on chosen days, monthly, yearly; every N; ending never, on a date
  or after N times), the dates it skips, reminders (minutes, hours or days before), status, busy
  or free, categories and the time zone. A repeating event offers "Edit this date" / "Edit all
  repeats" and "Delete this date" / "Delete all repeats". Shale stores reminders; it does not
  ring itself. Every date field has a small calendar button to pick the day from a month.
- "Restore last deleted" in the right-click menu of the empty space of the file list puts the item you moved to the trash last back at its original place. It never replaces anything: if the place is taken, the item stays in the trash and a message says so. (VAULT-020)
- Release downloads come with a SHA-256 checksum file (`shale-linux-x86_64.tar.gz.sha256`, on the "latest" build and every release page); check a download with `sha256sum -c`. It catches a damaged download, not a compromised release page.
- A separate "audit" job in the build workflow runs `cargo audit` on the dependencies, on every push and weekly; a new advisory fails only that job and never blocks a release build.
- **Release check**: a version is only tagged and built if it is valid semver and greater than the
  previous `v*` tag, `Cargo.toml` and `Cargo.lock` agree, and its `CHANGELOG.md` section is not
  empty and mentions every issue closed (`Closes #N`) since the previous tag. `make-release.sh`
  and both CI systems run the same script, `.github/check-release.sh` (BUILD-013) (#3).
- **Commit check**: CI on Forgejo fails for a commit pushed to `main` or in a pull request that
  names no issue and does not start with `no-issue:` (merge commits excepted; FORGE-008) (#4).
- **Keybindings in Settings.** Settings → "Keybindings…" lists every shortcut that can be
  changed (everywhere, file list, search results): press Change, then the new keys (several in a
  row make a sequence), Enter saves, Esc cancels; Reset and Reset all put the defaults back. A key
  that another action already uses is refused with a message naming that action. Only your changes
  are stored in the settings file (KEY-001 … KEY-009) (#10).
- The key tables of the README and the feature list are checked against the shortcuts the app
  really has (NAV-006).
- **Ctrl+Alt+H / Ctrl+Alt+L switch to the previous / next tab**, from the editor, the file list
  and the sub-apps alike. After the last tab comes the first. Both keys can be changed in
  Settings → Keybindings… (NAV-018) (#11).
- **Space Tab in vim-style normal mode** goes back to the note or sub-app (calendar, Feeds, …)
  this tab showed just before; pressing it again comes back, like vim's alternate file. It is
  the first key of a new "Vim-style editing, normal mode" list in Settings → Keybindings…; the
  vim keys themselves cannot be rebound there (NAV-019, EDIT-015). It does not work in the mail
  view and in drawings, where Space and f already have a meaning (#14).
- **Space f f in vim-style normal mode finds a file by name**: a window with a field and a list;
  typing filters (names only, fuzzy), ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the file in
  this tab with the keyboard in the editor, Esc closes. Unlike Ctrl+O it never creates a note
  (NAV-020, NAV-021). The quick switcher takes Ctrl+J / Ctrl+K too (#15).
- **Reorder tabs by dragging.** Drag a tab left or right; a marker shows where it lands. The
  order is kept for the next start (NAV-014, NAV-015) (#12).
- **Pin tabs.** Right-click a tab and choose "Pin tab" / "Unpin tab". Pinned tabs sit at the left
  with a small pin, can be reordered only among themselves, and are pinned again at the next
  start (NAV-016, NAV-017) (#13).
- **Search everything from the keyboard: `Space f g`.** In vim-style normal mode a window
  searches the text of your notes, your mail and the posts of your feeds at once. The hits are
  listed under Notes, Mail and Posts with a count each; type to filter, Ctrl+J / Ctrl+K or the
  arrows select, Enter opens a note at the found line, the mail in the mail view, or the post
  in its feed. `*` and `?` are wildcards (in mail and posts a wildcard makes the word a prefix).
  It is the keyboard way to the post hits that the sidebar search only offers with the mouse
  (NAV-022 … NAV-025).
- **Search in the posts of your feeds.** Every Mastodon and Bluesky post Shale loads is kept in a
  small search index on this computer (text, author name and handle, date, links, and the thread
  around it: the post it replies to, the quoted post, the link card, picture descriptions, who
  boosted it). Type in the sidebar search: matching posts are listed under the notes, and a click
  opens a new Feeds tab scrolled to the post, also when the feed does not hold it any more.
  `from:`, `text:`, `in:`, `link:` and `date:` limit a word to one part. The index keeps at most
  10 000 posts per account and nothing older than a year; removing an account removes its posts
  (SOC-030…SOC-036) (#17).
- **Reorder the icons of the ribbon by drag and drop.** Drag an icon of the left menubar to
  another place; a line shows where it lands. The order is kept in the settings and restored at
  the next start (LAY-001, LAY-002) (#20).
- **The right sidebar can be split into stacked views.** Each view has its own menubar of panel
  icons (Backlinks, Outgoing, Outline, Graph, Calendar) and shows one panel, so several are open
  at once. Icons move between menubars by drag and drop, also to and from the ribbon; icons that
  are no panels are refused with a no-drop pointer; the last icon leaving a view removes it. The
  layout is stored in the settings (LAY-003 to LAY-008, ADR-0014) (#21).
- **Drop zone for new views.** While a panel icon is dragged over the content of the right
  sidebar the area is highlighted; dropping makes a new view with its own menubar, Esc cancels
  (LAY-009, LAY-010). A panel icon in the ribbon shows its panel below the views (LAY-011) (#22).
- **Alternate view from everywhere: Ctrl+6** swaps the tab with the view it showed just before
  and back, also in the mail view and in drawings, where `Space Tab` cannot work because Space
  is the mark key and pans the drawing. It is a normal key action and can be changed (NAV-019,
  KEY-011) (#38).
- **Keybindings: Add and Clear.** Besides Change (replaces the keys of an action) there is now Add
  (one more key, the others stay) and Clear (the action has no key and stays so). (KEY-006, KEY-007)
- **A started key sequence shows in the status bar** (e.g. `Space f`) and is dropped after 1.5
  seconds without a key (KEY-009).
- **Drafts, Outbox, the row menu and the unsent-copies window work from the keyboard.** In
  Drafts and Outbox j / k move a marked cursor, Enter opens, `e` edits, `d` discards, Shift+S
  retries, `z` takes the selected mail back. The menu of a row opens with the Menu key or
  Shift+F10 and is driven with the arrows, Enter and Esc. Shift+U shows the sent mails that are
  only on this computer; there j / k, Enter (Upload again), `d` (Dismiss) and Esc work (MAIL-067,
  MAIL-068, MAIL-069; new Mail view actions, changeable in Settings) (#36).
- **Post search index: follow-ups.** Posts you save to the vault (the whole thread and the replies of
  other people) are now searchable. A reply is found by the text of the post it answers even when
  that post only arrives later. Opening a search hit whose post is gone from the index and the feed
  fetches it from Mastodon or Bluesky (an error message says when that fails). Settings → Feeds has
  a "Post search index" section with the number of indexed posts, **Rebuild** (indexes again what
  your open feeds hold) and **Clear…** (asks first) (SOC-037 … SOC-040) (#37).
- **Data usage dialog.** Settings → Data and caches → Data usage… (or Ctrl+Shift+D) shows what
  Shale keeps on disk: the vault by kind with its largest files and folders, and your mail,
  drafts, outbox, calendars and recovered notes outside it (read-only). Below it, every cache
  (social pictures, social post index, mail index and its backups, damaged mail index copies,
  tag words, keyboard debug log) with its size, age and last cleanup, and a Clear button; the
  mail index is rebuilt from the message files instead, after a question (USE-001 … USE-016) (#51).
- **Caches are cleaned up by themselves.** How long each cache is kept is a setting (social
  pictures 30 days / 500 MB, social posts 365 days / 10 000 per account, 3 mail index backups,
  debug log 10 MB, or "forever" / "no limit"). A background cleanup runs a minute after the
  start, then daily, and after a setting changes. Social pictures no longer grow without end
  (USE-020 … USE-024).
- **Development: `scripts/cross-check.sh` checks the macOS and Windows code on Linux.** It runs clippy for both systems and, with `--wine`, the Windows tests under Wine, so a problem that only shows up there is found before it is pushed. GitHub's macOS and Windows runs stay the real check (DEV-015, raba/shale#134).
- A middle click on the Mail, Feeds or Calendar icon of the ribbon opens that view in a new tab (raba/shale#156).
- **Markdown preview in the editor.** With "Markdown preview in the editor" in Settings →
  Appearance, or the eye icon in the top right of the editor, every line is drawn rendered:
  headings without their `#`s, bold, italic, strike and code without their markers, links as
  just their text. The line the cursor is in shows the raw markdown, and "Raw lines around the
  cursor" adds that many lines before and after; a selection shows all its lines raw. Embedded pictures on
  those raw lines show their link text too. The note
  stays editable, and links and task boxes keep working (EDIT-016, EDIT-017; raba/shale#161).
- The editor shows line numbers left of the text; the setting "Show line numbers in the editor" (Appearance) turns them off (raba/shale#162).
- Mail view: the buttons of the open mail have Alt shortcuts (Alt+I Import to vault, Alt+C Close, Alt+R Reply, Alt+A Reply all, Alt+F Forward, Alt+U Mark read/unread, Alt+G Flag, Alt+M Move, Alt+D Delete). Holding Alt underlines the letter on each button. The keys are listed and can be changed in the Keybindings tab of the settings (raba/shale#163).
- **Vim-style editing: `dd`, `u` and Ctrl+R.** In normal mode `dd` deletes the line and puts it
  on the clipboard (`p` puts it back below), `u` undoes the last step and Ctrl+R redoes it. Each
  note has its own history of the last 200 steps, shared by all tabs of the note and kept in the
  vault (`.shale/undo/`, not synced by git), also across restarts. They are one of the caches of
  Data usage ("Undo histories"): kept for 30 days by default (a setting, also "forever"), pruned
  by the daily cleanup, cleared on request (it asks first). A step is one command that changes the text, or a whole insert session from
  `i`, `a`, `o`, … to Esc; a change from outside (Alt+Up / Down, another tab, the file changed on
  disk) is a step of its own. `u` and Ctrl+R are now fixed keys that cannot be bound in the
  "Vim-style editing, normal mode" list (EDIT-020, EDIT-021, EDIT-022; raba/shale#174).
- **Pictures, PDFs, audio and video are imported as files, not notes.** Dropping an image, PDF,
  audio file (mp3, ogg, opus, m4a, aac, wav, flac) or video file (mp4, webm, mkv, mov, …) now gives
  one item of its own in the Import dialog (Image, PDF, Audio, Video); Import saves it in the media
  folder (`z_media/YYYY/MM/YYMMDD_name`) and makes no note. Dropped on a note, the note gets a link
  line for each file. An image dragged out of a web browser (Wayland) is saved from the picture the
  browser hands over, or downloaded from its address; before, it became a text note holding the
  address (IMP-031, IMP-032; raba/shale#175).
- **Pasting a picture or copied files into the editor saves them to the media folder.** Ctrl+V with
  a picture on the clipboard saves `Pasted image.png` there and inserts `![[z_media/…]]`; pasting
  files copied in a file manager (images, PDFs, audio, video) copies them to the media folder and
  inserts a link for each. Vim's `p` does the same, with the link on a line below the cursor line.
  Other text pastes as before (EDIT-019). GIF pictures are shown too.
- **Sound players in notes**: `![[song.mp3]]` shows a player with the cover from the file's tags (or a tile with the file name), a round play/pause button over it, a progress bar (click to jump, drag to seek on release), the time, a mute button and a volume slider, in the editor, the markdown preview and the reading view. mp3, ogg, oga, flac, m4a, aac and wav play (opus files are recognised, but the decoder is missing); one file plays at a time, and the sound stops when the note's tab is closed or the vault is switched. Seeking, starting and pausing fade over 5 ms, so there are no clicks. New settings: the opacity of the play button (Settings → Appearance, `media_play_button_opacity`, 0.6) and the volume of the players (`media_volume`, 0.8). Nothing ever starts by itself. Linux builds need `libasound2-dev` (raba/shale#178).
- **Video players in notes**: `![[clip.mp4]]` (mp4, m4v, mov, webm, mkv) shows the sound player's card with the first frame of the video as its picture (wide, 16:9, controls below). Playing shows the video; the sound goes through the same output and volume as sound files, one file plays at a time, a click on the progress bar restarts the video at that place, and a video without a sound track is timed by its frames. Videos are decoded by the **ffmpeg on your computer** (`ffmpeg` and `ffprobe`, started with an argument list and no shell). Without ffmpeg the video shows a tile with a play mark, a click opens the file in your system's video player, and Settings → Appearance says that ffmpeg is missing and where to get it (raba/shale#179).
- Git sync can store media files (audio and video) with Git LFS, so they stay out of the git history. With `git-lfs` installed, a repository Shale creates does it from the start, and Settings → Git sync has a "Store media with Git LFS" button for an existing one; it converts the media already tracked in one commit (history is not rewritten). The settings say whether git-lfs is installed and whether the repository uses it. Every sync uploads the LFS files before it pushes, a conflict copy of a media file is stored in LFS too, and a vault that uses LFS is not synced (nothing is committed) on a computer without git-lfs (raba/shale#180).
- Media stored with Git LFS are downloaded on demand. A sound or picture embed whose file on this computer is still an LFS pointer shows "Not downloaded (12 MB) — Download" instead of failing to decode; pressing play downloads the file first and then plays it. The download runs in the background (`git lfs pull` for that file only), shows a spinner, and says in words why when it fails. Settings → Git sync has "Download media files after every sync" (vault setting `sync.lfs_download`: `on_demand`, the default, or `always`) (raba/shale#181).
- **A user guide you can open in Shale.** `docs/userguide` is a vault: a "Start here" note links to one note per
  feature (notes and links, editor, tasks, drawings, calendar, mail, feeds, import, git sync, window layout,
  keys, and more), written with the features it explains, with all the screenshots and a settings reference
  (every setting, its default, and whether it is global or in the vault). Open it with
  `shale --vault docs/userguide` (#182).
- **The user guide reaches you: F1 or Settings → Help → Open user guide.** The guide is a Shale
  vault; Shale copies it once per version into its data folder (a copy you edited is never
  overwritten) and opens it at "Start here". The release archive has it as `docs/userguide/` next to
  the program, and each release also has `shale-userguide-<version>.zip` with its `.sha256` file
  (DEV-020, raba/shale#182).
- **`shale --record clip.webm`** runs a `--keys` script like `--screenshot` and records it as a short
  video (VP9, 30 frames per second of app time, so a clip is the same on a slow and a fast machine),
  encoded by the system `ffmpeg`; a missing ffmpeg is reported in words (DEV-019, raba/shale#184).
  The clips of the user guide are made by `docs/screenshots/regen-clips.sh` from `docs/screenshots/clips.conf`.
- **Tables are drawn as real tables.** In the reading view, and in the editor with the Markdown
  preview on (wherever the cursor is not), a Markdown pipe table is a grid: borders, a bold
  header row, the `|---|` line hidden, columns aligned as `:---`, `:---:` and `---:` say, the
  inline formatting (code, bold, links, tags) in the cells, and long cell text wrapping inside
  its own column instead of across the row. Click the grid or move the cursor into it to edit
  the raw rows. The PDF export draws the same grid: wrapped cells, a filled bold header row,
  borders, alignment, and when a table runs over a page break it continues on the next page
  with the header row repeated. The export dialog has a new checkbox, "Export plain text
  (tables as raw text lines)", off by default (EDIT-024; raba/shale#189).
- Ctrl+J / Ctrl+K move through the `[[` suggestions and the `[!` file picker like ↓ / ↑ (raba/shale#200).
- **Vim-style editing: `gt` follows the link under the cursor.** In normal mode, `gt` (go to)
  does what a click on the link does: a `[[note link]]` opens or creates the note, a Markdown
  link or web address opens in the browser. Off a link it does nothing; `gg` is unchanged
  (EDIT-023; raba/shale#202).
- **User guide: notes for sounds, videos and Git LFS.** New note "Sounds and videos" (players, ffmpeg, downloading media from other computers) and a Git LFS section in "Git sync", with the demo sound and a clip played in the guide itself.
- **An app icon.** Three slate plates stacked on a dark square, in the title bar, the taskbar and
  the macOS Dock. The Linux download has `shale.desktop` and `shale.svg`, so Shale can be put in
  the app menu and shows its icon in the Wayland taskbar too (the README says how; NAV-031;
  raba/shale#213).
- Pictures and videos in the editor can be resized with draggable edges and corners (the width is kept in the link, `![[photo.png|300]]`, and the reading view honours it), and have a round **100%** button for the natural size and a fullscreen button (raba/shale#223).
- A formatting toolbar, centred above the text of a note: bold, italic, strikethrough, highlight, inline code, headings, lists, tasks, quotes, code blocks, rules, tables, note / web links, embeds and "Clear formatting", one undo step per click, also with the Markdown preview and vim-style editing; Settings → Editing, "Show formatting toolbar" and "Toolbar button size" (raba/shale#225).
- Coloured highlights and font colours, stored as `<mark style="background: #FFFF00;">` and `<span style="color: #FF0000;">` (the convention of Obsidian's colour plugins), with a palette in the toolbar (four neon highlights, your own font colours in Settings → Editing, a colour picker, "No highlight" / "Default colour"). They show in the editor, the Markdown preview, the reading view, tables and the PDF export (raba/shale#226).
- **Buttons on the table grid.** Hover a table in the editor's Markdown preview and small
  buttons add a column at the end, add a row at the end, and, for the hovered row and column, add
  one after it or delete it. Only the table's lines change, as one undoable step; other cells keep
  their text exactly, and the table stays a grid (EDIT-025; raba/shale#228).
- **Ctrl+Z steps for programmatic edits.** Without vim-lite, a table button, a line move
  (Alt+Up/Down), a list continued with Enter and a Tab indent are each one Ctrl+Z step, no
  longer merged with the typing around them or with each other (EDIT-025).
- **Edit table cells in the grid.** With the Markdown preview on, a click on a table cell edits
  that cell in place instead of turning the table into raw markdown. Tab / Shift+Tab and
  Enter / Shift+Enter move between cells, Esc leaves; the whole editing is one undo step. A new
  setting "Edit table cells in the grid" (on by default) and a button at each grid switch between
  this and the raw markdown, globally or per table (EDIT-030; raba/shale#234).
- **Resize tables.** Drag the column and row borders, or the right / bottom edge and the corner
  for the whole table; a double click on a border shares the width (height) equally among all
  columns (rows). The sizes are used in the editor and the reading view (EDIT-031).
  Per-table settings and sizes are kept in an HTML comment line above the table.
- **Arrow keys between table cells.** While a cell is edited in the grid, → at the end of its
  text goes to the next cell and ← at its start to the previous one (rolling over to the next /
  previous row); ↑ on the first line and ↓ on the last line go to the cell above / below
  (EDIT-030; raba/shale#235).
- **Syntax highlighting.** Fenced code blocks are coloured by the language on the opening fence
  (```` ```js ````, ```` ```rust ````, `python`, `sh`, `json`, …, with aliases) in the editor and the
  reading view; unknown or missing languages stay plain. Inline code colours its strings, numbers
  and calls. Results are cached (EDIT-032; raba/shale#236).
- **The toolbar formats the cell being edited.** While a table cell is edited in the grid, the
  formatting toolbar's inline buttons (bold, italic, strikethrough, highlight, code, links, colours,
  clear formatting) act on the selection or cursor inside the cell, and the result is part of the
  cell's undo step; the line buttons (headings, lists, quote, code block, rule, table) and the embed
  are greyed out until the editing ends (EDIT-030; raba/shale#237).
- **Fonts and text sizes per area** (#240). Settings has a new Fonts tab: dialogs and menus, the raw
  markdown text of the editor, the rendered text (markdown preview and reading view), mail,
  calendar and feeds each have a font and a size. "Use the same font and size everywhere" (on by
  default) shows one row for all of them. A font is chosen from the fonts installed on the computer
  in a list you can search by typing (arrow keys and Enter, Esc to close). Changes show at once,
  also when you edit `[fonts]` in the settings file. A font that is no longer installed is replaced
  by Shale's own, with a message.
- **Select several table cells.** While a table is edited in the grid, Shift+arrow keys select a
  rectangular block of cells from the edited cell (Shift+Right then Shift+Up: 2 by 2). The selected cells are
  highlighted; the toolbar's inline buttons format all of them, Delete and Backspace clear them (each one undo
  step), typing goes into the top left cell of the block, and a plain arrow key or Esc ends the selection
  (EDIT-030; raba/shale#243).
- **TypeScript, TOML and about 140 more languages in code blocks.** `ts`, `typescript` and `tsx`
  now have their own grammars (they were coloured as JavaScript), and `toml`, `kotlin`, `swift`,
  `dockerfile`, `ini` and many more work too (EDIT-032; raba/shale#244).
- **Split the middle area into panes side by side.** Ctrl+\ (or "Open in split right" in the tab
  menu) opens a tab in a new pane to the right; every pane has its own tab bar, tabs and
  back / forward history, and one pane has the focus (accent line under its tab bar; click into a
  pane or use Ctrl+Alt+→ / Ctrl+Alt+←). Opening, closing, switching tabs, back / forward and the
  reading view act on the focused pane, renames and deletes follow in all panes. The borders
  between panes drag (240 points minimum), closing a pane's last tab (or Ctrl+Alt+W) closes the
  pane, and panes, tabs, focus and widths come back at the next start (NAV-032 to NAV-036,
  raba/shale#249).
- **Link preview in vim-style normal mode.** With the cursor on a link, **Shift+K** (definable,
  for example as Space Space) shows a floating preview like neovim's hover: a note as the reading
  view draws it (from the heading of the link), "Note does not exist yet" for a missing one, a
  picture, or for a web link the page's title, description and the start of its text, loaded in
  the background. Shift+K again moves the keyboard into the preview (j / k, Ctrl+D / Ctrl+U scroll,
  Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc gives the
  editor the keyboard back); moving the cursor or typing closes it (EDIT-033…036, #250).
- **Security findings become issues**: a daily check (`cargo audit`) opens a GitHub issue labelled
  `security` for every advisory on a dependency (vulnerable, unmaintained, unsound or yanked) and
  closes it once the dependency is fixed. `SECURITY.md` says how to report a vulnerability
  privately (#257).
- **Font samples in Settings → Fonts.** Every font row shows a sample text in its font and size,
  with a bold and an italic word, so all chosen fonts can be seen at once. While the font list of a
  row is open, its sample shows the font under the mouse or the arrow keys before you pick it; the
  list opens on the chosen font (CFG-040, #275).
- **Mail: sent mails that never reached the server's Sent folder are shown, and you can upload them again.** When Shale restores its mail index from a backup, or the server refuses to take the copy of a mail you sent, that copy used to stay on this computer only, without a word. The mail view now says "N sent mails are only on this computer"; "Show…" lists each (subject, recipient, date, account) with "Upload again" and "Dismiss", and "Upload all again" and "Dismiss all". Uploading goes through the normal queue and never makes a second copy, however often you click. "Dismiss" keeps the mail in the local Sent folder and stops the notice; nothing is ever deleted. Mails of an account that is not set up any more can only be dismissed (MAIL-210, MAIL-211, MAIL-212, MAIL-213, MAIL-214).
- **Passwords and tokens go to the macOS Keychain and the Windows Credential Manager**, as they go
  to the system keyring on Linux. CalDAV and mail passwords, OAuth2 tokens and the sign-ins of
  Mastodon and Bluesky are no longer kept in the settings file on those systems (SEC-024, SEC-025).
  The settings dialog names the store in use. If the Keychain cannot be reached, or you cancel its
  unlock prompt, Shale tells you and keeps the secret for this run only (SEC-011).
- **Shale builds and passes its tests on macOS and Windows.** Linux (Wayland and X11), macOS
  (Apple silicon) and Windows (x86_64) are supported; GitHub builds, lints and tests the macOS and
  Windows versions for every change to `main`. There are no release downloads for them yet: build
  from source (README, "Supported systems"). Passwords and tokens go to the
  Keychain and the Credential Manager there (see the entry for raba/shale#26).
- Opening links and files on Windows no longer goes through a command shell; it uses the system's
  "open" call with the same checks as on Linux and macOS (SEC-005, SEC-022).
- A file that must not be replaced (new notes, the media folder, renames) is never replaced on macOS
  and Windows either (DATA-013).
- A missing GnuPG now says where to get it (Homebrew, Gpg4win, your package manager).
- **Releases are attested.** Each release tarball and its `.sha256` file get a build-provenance
  attestation signed through Sigstore, without a key: it shows that the file was built by Shale's
  release workflow on GitHub from the version tag. Check it with
  `gh attestation verify <file> --repo rabalyn/shale --signer-workflow rabalyn/shale/.github/workflows/build.yml --source-ref refs/tags/v<version>`;
  the release page also has the attestation as `<file>.sigstore.json` (BUILD-014).
- **Numbered headings.** The headings of a note (`##` and deeper) show a number before their text, `1`,
  `1.1`, `1.2`, `2`, drawn muted in the editor, the Markdown preview, the reading view, the outline, the
  link preview and the PDF; the number is never written into the file, and the first `#` heading, the
  note's title, has none. Settings → Editor → "Number headings" (on by default) turns it on or off, and a
  note's front matter `numbered-headings: true` or `false` overrides it for that note (LINK-014, CFG-042, #283).
- **Table of contents and bookmarks in the PDF.** The note export has a "Table of contents" checkbox (on):
  after the title, a *Contents* page lists the `##`–`####` headings with their numbers and the pages they
  start on, each entry a link; the text starts on the next page. The PDF also has bookmarks for the
  `##`–`######` headings, nested by level (PDF-008, #283).
- **Heading completion.** After `[[Note#` or `[[#` the link suggestions list the headings of that note, with
  their numbers, and insert the heading's plain text (LINK-016, #283).

### Changed

- **Development moves to Forgejo** (`git.home.rabalyn.de/raba/shale`): its issues are now the one
  list of open work and finished work closes its issue; GitHub will only distribute releases
  (ADR-0013, FORGE and DEV rules in the engineering spec) (#1).

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
- **Calendar: "Delete event" asks first and keeps a copy.** The event popup shows "Delete “Title”?"
  (or "… with all its repeats?" / "Delete only this date …") with Delete and Keep. Before an
  event or one date of it goes, the whole event is written to `calendar-trash/<date>/` in the data
  folder; if that copy cannot be written, nothing is deleted. The message says where the copy is
  (CAL-011, CAL-017).
- The vault trash keeps where an item came from: it goes to `.trash/<date>/<its path in the vault>`, so two notes of the same name from different folders no longer get mixed up. A name already taken that day gets ` (2)`, ` (3)` … before the extension; nothing in the trash is ever replaced. Entries directly in `.trash/` from earlier versions stay where they are. (VAULT-017)
- Git sync refuses a remote address with a password in it (use a credential helper or an SSH key)
  and `http://` or `git://` addresses unless the host is this computer. A password in an existing
  remote is shown as `***` in the settings and in error messages.
- **Builds: day-to-day CI moved to a self-hosted Forgejo; GitHub builds only releases.** Every
  push and pull request is now formatted, linted, built, tested, packaged with a checked
  `.sha256` file and audited (`cargo audit`, also weekly) on the development server. GitHub runs
  only for version tags: it builds and tests the tag again and publishes the release with its
  `.sha256` file. The rolling "latest" pre-release of `main` is no longer updated; to try an
  unreleased version, build it from source. `make-release.sh` pushes the commit and the tag to
  both servers (BUILD-002, BUILD-009 … BUILD-012) (#2, #52).
- **Internal: the code is split into feature folders and `ShaleApp` has sub-states** (#5, DEV-008).
  The calendar, mail, drawing, import and PDF files moved into `src/calendar/`, `src/mail/`,
  `src/drawing/`, `src/import/` and `src/pdf/` (each with a parent file and its tests next to it), so
  `main.rs` lists only top-level modules. The window state `ShaleApp` went from 66 to 44 fields: drag
  and drop, the calendar views and jobs, tag learning, scripted input and the open tabs now live in
  their own types (`DragState`, `CalendarUi`, `TagLearner`, `ScriptRun`, `TabBar`). No change in
  behaviour.
- **Internal: the last two global clippy allows are gone.** `type_complexity` and `too_many_arguments` are
  linted like everything else; the 22 places that tripped them now use named type aliases and small
  parameter structs. No change in behaviour (DEV-002).
- **Housekeeping: dead code removed, CI fails on an unused dependency.** Removed an unused
  function and redundant `allow` attributes (DEV-002, #6).
- **CI: coverage and the real-server tests.** Two new jobs on the development server, both separate
  from the build. `coverage` measures line coverage with `cargo llvm-cov`, prints it per file and
  fails when the total falls more than 0.5 points below `.forgejo/coverage-baseline`. `env-tests`
  starts the CalDAV and mail test stacks in podman and runs `cargo test -- --ignored caldav_env`
  and `mail_env` on pushes to `main`, on pull requests that touch the calendar or mail code or the
  test environments, and weekly; on a runner that cannot run containers it says so and is skipped.
  `docs/coverage.md` lists the coverage per file and the biggest gaps (#33).
- **CalDAV is now tested against a real server.** `tests/caldav-env/` starts Radicale behind TLS
  with a seeded user and calendars (single, repeating with a skipped and a changed date, reminder,
  all-day, an empty one for moves); `cargo test -- --ignored caldav_env` checks loading, creating,
  editing, moving, deleting, a change made on the server meanwhile and a wrong password against it.
  For developers only: the app itself is unchanged (CALENV-001 to CALENV-003, DEV-005) (#8).
- **The screenshots in the docs can be made again with one command.** `docs/screenshots/regen.sh`
  runs the recipes of `docs/screenshots/recipes.conf` (one `--screenshot --view --keys` run per
  picture, on a private X display) and writes all 27 pictures, or only the names given. New
  pictures: the right sidebar split into two views and a pinned tab with its menu; the mail view
  now shows an open message, and the other pictures were made again. For developers only
  (DEV-007) (#9).
- **`--keys` can use the mouse.** New steps: `rclick`, `hover`, `drag:from>to`, `wait`, `waitfor`
  and `dump`; a step can aim at a drawn text (`click:@Sync`) instead of coordinates. A step that
  does not parse, or a text that is not on screen, now stops with an error instead of making a
  wrong picture. `--size WxH` sets the window size. The window of a screenshot run counts as
  focused, which fixes lost key presses (for example the `[[` suggestions) on an X display
  without window manager (BUILD-007).
- **Pull requests start from a checklist** (`.forgejo/pull_request_template.md`) for the doc,
  screenshot, spec and changelog duties of DEV-006 (DEV-009).
- A key binding now only fires with exactly its modifiers: Ctrl+Alt+L no longer also triggers
  Ctrl+L, and Shift+J no longer counts as j.
- **The Feeds button in the left bar always opens a new Feeds tab** where you pick the account and
  the feed, so a new feed can be added to your tabs. It no longer jumps to a Feeds tab that is
  already open (SOC-022) (#19).
- **The mail view is keyboard first.** The pane that has the keyboard (folders, message list,
  message) is framed in the accent colour, and Ctrl+H / Ctrl+L move between the panes like they
  move between the file list and the editor (from the folders Ctrl+H goes on into the left
  sidebar). j / k act in the focused pane only. New keys cover what needed the mouse: sync
  (Ctrl+R), accounts (`a`), load from the server (Shift+L), attachments (`]`, `o`, Shift+O),
  search scope (`t`) and undo send (`z`). All mail keys are now in Settings → Keybindings…
  (scope Mail view) and can be changed (MAIL-061 … MAIL-066, KEY-002) (#18).
- The text tabs of the right sidebar are replaced by icon menubars.
- **All tabs come back in place, with their pins.** The open tabs (notes, drawings and Feeds
  tabs) are saved as one ordered list, so a restart restores every tab where it was, pinned tabs
  pinned and left-most; before, Feeds tabs came back after the notes and unpinned. Older session
  files are migrated when loaded (NAV-026) (#32).
- **Right sidebar: resizable views.** The border between two views can be dragged to change
  their heights; the heights are kept (LAY-012) (#39).
- **Right-click menu on menubar icons**: remove an icon from the ribbon or a view, restore
  removed icons, or reset the whole layout to the default. The last ribbon icon stays so the menu
  can always be reached (LAY-013, LAY-014).
- The panel shortcuts of the ribbon (local graph, calendar) have glyphs of their own instead of
  those of the Graph and Calendar buttons (LAY-015).
- **Keybindings is a tab of Settings** (General | Keybindings) instead of a separate dialog, and
  the columns line up across the groups (KEY-006) (#34).
- **Documentation screenshots no longer change from run to run.** `docs/screenshots/regen.sh`
  freezes the clock and the build stamp of the pictures (`SHALE_FAKE_NOW`, only with
  `--screenshot`), so diffs show only real changes. In `--keys` scripts an icon button is
  found by its tooltip (`@Outline of the note`) and a tab as `@Tab 1`. The graph now places
  missing notes in the same order on every start (DEV-007, BUILD-007) (#93).
- **A clear notice that Shale is built by AI.** The README now opens with a warning box, and
  every release page starts with the same notice: Shale's code, tests and docs are written by AI
  coding agents (agentic coding), not by hand; the owner sets the goals and specs, reviews and
  tests. People who do not want to support software made this way can see it before they install
  or contribute. CI fails when the notice is missing (BUILD-015, raba/shale#47).
- **Mail view: the keys are shown.** The entries of the row menu show their key on the right, and
  the tooltips of the buttons (toolbar, marked mails, outbox, drafts, unsent copies) name the key; the buttons of the outbox detail (Retry, Edit, Discard, Undo send) also show it on the button.
  They follow Settings → Keybindings: a changed key shows the new one, an action without a key
  shows none. A row of Drafts or Outbox that Tab moved to is now framed (MAIL-072, MAIL-073).
- **Tests: the file-safety paths that never ran are now tested.** Moving a file or folder without
  replacing anything on systems without `renameat2`, link rewrites that cannot finish (a note
  changed or unreadable meanwhile, a drawing that cannot be written), the settings file (save,
  load, broken, newer, not writable) and the reload of the vault settings (DATA-004, VAULT-016,
  CFG-001, CFG-004, CFG-006, CFG-007, DATA-009) (#56).
- **Tests: IMAP answer parsing, `APPEND` failures and the production connectors.** The answer
  parser is tested with odd but legal and with broken server output (cut off at every byte, NUL
  bytes, nested literals); `APPEND` refusals after the literal and cut-off connections; the retry
  of a refused OAuth2 token and the settings of the IMAP and SMTP connectors, against a refused
  loopback port and the pretend servers (MAIL-220 to MAIL-223) (#57).
- **Tests: the move fallbacks for servers without MOVE or UIDPLUS are covered.** A copy the
  server refuses, a mark or expunge that fails, a copy found only by Message-ID and "delete
  forever" without UIDPLUS are checked: the original is never removed before the copy is
  verified, and mail the person did not delete is never expunged (MAIL-103, MAIL-105) (#58).
- **More tests for CalDAV and local calendar failures** (#59). A scripted HTTP server checks what
  the CalDAV client says for redirects (301, 302, 307, 308, also to another server) and for 401,
  403, 404, 412, 5xx and answers that are not HTTP, when finding calendars, saving and deleting;
  the calendar store is tested for local files that cannot be written, one date of a repeating
  event removed, and a CalDAV refresh that fails or finds a changed calendar (CAL-019 to CAL-022).
- Tests for the words of the mail status strip and of failed signing, encrypting and decrypting:
  expired, revoked and unknown signers, a tampered signature, expired signatures, refused
  recipients and the structure checks of odd mails, with real keys made by GnuPG (MAIL-172,
  MAIL-174, MAIL-182, MAIL-193, MAIL-195, MAIL-197) (#60).
- **Drops from other programs can be tested without a compositor.** The part of `wl_drop.rs` that
  does not talk Wayland (which kind to accept, the drag state, turning the read data into a drop,
  ignoring the second copy of a drop) is plain code with unit tests, and the window tests feed
  drops to the window (a folder, a note, the editor, nowhere; hints; unusable drops; moving in the
  file list). For developers only (DEV-012, IMP-002, IMP-012 to IMP-017) (#76).
- **`--keys` can drop files at a place.** New form `drop:T:/a;;/b` (T is `x,y` or `@label`) drops
  files there the way another program does, so the folder, note or editor under it decides where
  the notes go; `drop:/path` works as before (BUILD-007) (#61).
- Tests for the drawing canvas (selecting, moving, resizing, drawing with every tool, panning,
  editing text; DRAW-019) and for the dialogs, the task list and the file tree (VAULT-023,
  VAULT-024, TASK-005) (#62).
- Internal: tests now run the real opener against a recording fake `xdg-open`, and the OAuth2
  and Feeds sign-ins against a pretend browser (approve, deny, wrong state, never answers) and a
  pretend Mastodon and Bluesky server (SEC-006, MAIL-080 … MAIL-083, SOC-001 … SOC-004).
- Internal: the real-opener tests now run on macOS (the recording fake replaces `open`) and on
  Windows (the fake takes the place of the `ShellExecuteW` call), not only on Linux (SEC-006,
  SEC-022).
- **A second start hands its folder or file to the running Shale.** Opening a note from a file
  manager (or starting `shale <folder>`) while Shale runs now brings the window forward and opens
  the file in a tab, or the folder as the vault, instead of showing "already running" (DATA-015) (#78).
- **Calendar connection problems are named in words.** A redirect loop, a server that cannot be
  found or reached, a timeout, an untrusted certificate, an empty reply or an answer cut short now
  say so, instead of "status 302" or curl's own text (CAL-030).
- **A red macOS or Windows run on GitHub now comes back to Forgejo as an issue.** The macOS and
  Windows jobs run on GitHub for every `main` that passed Forgejo's CI, and nobody looked there.
  A scheduled Forgejo workflow (`github-report`, every 30 minutes) reads the runs from GitHub (the
  repository is public, so it needs no GitHub login) and opens the issue "GitHub CI red on main:
  windows failed at 1a2b3c4" with the commit and the links of the run and the failed jobs; further
  red runs are added to it as comments, and it is closed when `main` is green again. Running the
  tests now needs `jq` besides `curl` (BUILD-020; raba/shale#94).
- **The buttons of the long dialogs stay in view.** Mail accounts, Calendars, Import and Data usage
  now keep their buttons (Done, Cancel, Import, Refresh, and the "Yes, clear" question of Data
  usage) in a footer below the content, like Settings: in a short window the content scrolls and
  the buttons no longer have to be scrolled to. The footer takes the height it measured in the
  frame before, so it is not clipped when it holds more text (NAV-028, NAV-029; raba/shale#97).
- **Event dialog: the end follows a changed start.** Typing or picking a later start date or time
  moves the end by the same distance (whole days for an all-day event), so the event keeps its
  length instead of showing "The event must end after it starts". Changing the end never moves
  the start (CAL-031).
- **Internal: `ShaleApp` groups more of its state** (#119, DEV-008). The search box, the file list,
  the two graphs, screenshot mode and the recovery folder now live in their own types (`SearchBox`,
  `FileTree`, `Graphs`, `ScreenshotRun`, `RecoveryState`), and the shown left panel is part of
  `SideState`. The window state went from 44 to 34 fields. No change in behaviour.
- **The Event, Feed accounts and Export to PDF dialogs keep their buttons in view too.** Save (or
  Move here) and Cancel of the event dialog, Sign in and Close of Feed accounts, and Export and
  Cancel of Export to PDF now sit in a footer below the content, like the other long dialogs: in a
  short window the content scrolls and the buttons stay in view. The error of the last try and the
  "Saving to …" line stay next to them, and Ctrl+Enter still saves an event (NAV-028;
  raba/shale#126).
- The made-up mail of `shale --screenshot --view mail` holds a not-sent and a may-have-been-sent outbox mail, so the Outbox detail can be pictured with the new `outbox-failed` and `outbox-unsure` recipes (raba/shale#138).
- The window has a smallest size of 1280x720 points (before: 720x480); it cannot be resized below it, and `--size` takes no less (raba/shale#143).
- **Development: tests no longer start the machine's GnuPG.** In a test build the default GnuPG names programs that do not exist, so an app built by a test cannot run the real `gpg` or `gpgsm` against the tester's own `~/.gnupg`; tests that need GnuPG use a throw-away home (DEV-017, raba/shale#145).
- **Plans for media and the user guide** (no change to the app yet): ADR-0015 decides an inline
  audio and video player (own pure-Rust audio engine, video through the system ffmpeg) and Git
  LFS for media files set up by the git sync; the user guide becomes a Shale vault. Spec rows
  MEDIA-001…011, SYNC-016…022, DEV-018 (raba/shale#178…#182).
- **The screenshots moved** into the guide's media folder, `docs/userguide/z_media/screenshots/`, so a
  vault can show them; `docs/screenshots/regen.sh` writes them there and the recipes stay in
  `docs/screenshots/`. For developers only (DEV-007, DEV-018).
- A test fails when a section of `docs/FEATURES.md` has no guide note or a setting is missing in the
  settings reference (DEV-018).
- **The user guide is built into the program.** F1 (or Settings → Help → Open user guide) unpacks
  the guide into Shale's data folder (once per version, a copy you edited is never overwritten)
  and opens it as a vault, in every build on every system. Shale no longer looks for it next to
  the program or in the source tree (DEV-020, #187).
- **Changes land locally.** `scripts/land.sh <branch>…` merges a branch into `main`, runs the
  commit check, fmt, clippy, the tests (and the real-server suites when CalDAV or mail code
  changed) and pushes only when all pass, one landing at a time. CI on Forgejo runs for `main`,
  tags and the weekly schedule only, no longer for every pull request, which had made merges wait
  for a queue on the single runner. A merge conflict, in docs or code, is resolved in the branch
  and landed the same way. For developers only (FORGE-002, FORGE-010, FORGE-011, ADR-0016) (#210, #215, #217).
- Embedded pictures, drawings, PDFs, videos and sounds have their link as a caption below them in the editor (a click opens the file; `![[photo.png|Sea at dusk]]` sets the words), and the PDF export draws the same thumbnails with the same captions instead of printing the raw link, now also for the first frame of a video and the cover of a sound (raba/shale#224).
- The PDF export draws embedded pictures, drawings and PDF pages as big, in proportion to the text, as the editor shows them (a picture made smaller stays smaller), and sounds and videos as their player card instead of the bare cover; the editor and the export share one sizing rule and the export measures against the real width of the editor's text column (raba/shale#229).
- **The PDF export uses the column widths and row heights stored with a table.** Sizes you set
  by dragging a grid's borders are now used in the PDF: the widths as in the editor (every
  column at least 24 points, scaled down together only when wider than the page's text), a
  stored row height as the least height of its row. A table whose sizes do not match falls
  back to the automatic widths (EDIT-031, PDF-001; raba/shale#238).
- **Settings is sorted into tabs by domain** instead of one long General page: Vault, Editor, Appearance, Sync, Calendar, Mail, Feeds, Data, Help and Keybindings, in a list on the left. It opens on the short Vault page, and again on the tab you used last. The fetch interval and mark-as-read delay of mail, the week start of the calendar and the check interval of the feeds can now be set right on their tabs (raba/shale#239).
- The "Text size" slider of Settings → Appearance is replaced by the Fonts tab: your old size is now
  the size of the two editor areas (settings version 2).
- **The settings are a text file you can edit.** Shale's own settings moved from
  `~/.config/shale/config.json` to `config.toml`, a plain text file with a description above
  every setting. **Settings → Open settings file** opens it in a tab of Shale (it is not a note
  and not part of any vault); saving the tab uses the new values at once, and a change made with
  another editor is picked up within a second. Text with an error is never saved over the file:
  the tab says what is wrong and your text stays. The first start after the upgrade turns the old
  `config.json` into `config.toml` and renames it `config.json.migrated` (raba/shale#241, CFG-009
  to CFG-014).
- **The PDF export prints code as you see it.** Fenced code blocks and inline code have the syntax
  colours of the reading view, on the same dark code background (PDF-007; raba/shale#242).
- Shift+K is no longer a fixed key of vim-style normal mode (plain k still moves up).
- **Development: `scripts/land.sh` runs the macOS and Windows clippy of `scripts/cross-check.sh` before a change lands.** Code that is only used from Linux-only code no longer reaches `main` and breaks GitHub's macOS and Windows runs (BUILD-004, DEV-015, raba/shale#251).
- **Real bold and italic in a chosen font.** With a system font chosen in Settings → Fonts, bold,
  italic and bold-italic text in the editor, the reading view and the link preview is drawn in the font's
  own bold and italic faces where it has them, instead of a slanted or spaced-out imitation of the
  regular face. Fonts with only a regular face keep the imitation (CFG-039, #253).
- **Issues are now on GitHub, the public home of Shale** (<https://github.com/rabalyn/shale/issues>).
  The earlier issues and pull requests of the development server were imported with their
  numbers, so `#N` in commits and notes still means the same issue. Development and day-to-day CI
  stay on the self-hosted Forgejo; GitHub builds only releases now: macOS and Windows are built
  and tested for each release instead of for every change (#256, ADR-0017).
- **Ctrl+H / Ctrl+L walk through the split panes.** Like vim's window moves: Ctrl+H goes from a
  pane into the pane on its left and from the first pane into the left sidebar, Ctrl+L from the
  sidebar into the first pane and on to the right (it stays in the last pane). With one pane
  nothing changes (NAV-011, raba/shale#263).
- **The link preview is bigger and can be resized.** It is 640 x 480 points now (was 520 x 400), and
  its four edges and corners can be dragged to change the size; the preview stays open and the cursor
  does not move. The size is a setting that every preview uses and that is kept for the next start
  (`link_preview_width`, `link_preview_height`, Settings → Editor → "Link preview size", with a
  Reset button). The scrolling keys follow the new height (EDIT-037, #264).
- **PDF export reads the bundled fonts with `skrifa` instead of the unmaintained `ttf-parser`
  (RUSTSEC-2026-0192).** The PDF looks the same. `fontdb` is updated to 0.24, which no longer
  depends on `ttf-parser` either (raba/shale#268).
- The settings dialog's text without a keyring is specific to the system: it points to `secret-tool`
  only on Linux (SEC-003, SEC-021).
- The media folder's date, the conflict-copy stamp and event times use the computer's time zone
  through the Windows time zone call there, and through the C library elsewhere (CAL-018).
- Release builds on macOS refuse core dumps and debugger attach, like Linux does (SEC-019, SEC-023).
- **Passwords and tokens are wiped from memory.** A password, OAuth2 token or client secret you
  type or load now lives in a locked memory area that is cleared as soon as it is no longer
  needed, instead of staying in freed memory. Password fields no longer keep an undo history of
  what you typed, the short-lived copies made while signing in are cleared too, and release
  builds refuse core dumps and memory reads by other programs. Where your system does not allow
  locking memory (RLIMIT_MEMLOCK), secrets are still wiped, but could reach swap: use encrypted
  swap next to full-disk encryption (SEC-014 to SEC-020).
- **Heading links scroll to their heading.** `[[Note#Heading]]`, `[[#Heading]]`, `[text](Note.md#Heading)` and the
  outline now open the note with the heading at the top of the view and keep the tab's mode: in the reading
  view the view scrolls (before, a heading link always switched to the editor and the reading view dropped
  the heading). The heading is found ignoring case, spaces and inline markup; a heading that is not there
  opens the note at its top and says so (LINK-015, #283).
- **The tab list of Settings is as wide as its longest name, and its border can be dragged.** With
  a bigger font "Appearance" and "Keybindings" no longer wrap: the list measures its names. Drag
  the border between the list and the page to set the width (60 points to half of the window;
  kept as `settings_tab_width`), double-click it to make the width automatic again (CFG-041;
  raba/shale#284).

### Fixed

- **Only one Shale runs at a time**: starting Shale a second time now shows a message and does not open, so a mail can no longer be sent twice and settings changed in one window can no longer be overwritten by the other.

- **Calendar: saving an event no longer drops what Shale does not show.** Before, changing an
  event rewrote it from the few fields Shale knew: reminders, the answers of invited people,
  categories, the status and every field of other programs were lost on the server, a changed
  single date of a repeating event came back as a duplicate, and a repeating event was stored
  in UTC, so other programs showed it an hour off in the other half of the year. Now every line
  that was read and not changed is written back as it was, times keep their zone, and a local
  calendar file keeps its tasks and other entries. A repeating event made by an older Shale is
  repaired by opening it with "Edit all repeats" and saving it once.

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
- **Calendar: a CalDAV save or delete without a version tag no longer overwrites a change made on
  the server.** Shale first fetches the event and compares it with what it read; if it differs, the
  save or delete is refused with "the event was changed on the server meanwhile" (CAL-016).
- **Mail: changing the server or user name of an account** no longer applies the old mailbox's local copy and queued actions to the new one. The account becomes a new mailbox with an empty local copy; the old mail stays on disk and the dialog's note says where. Queued actions of an account that is no longer set up are never sent.
- **Mail: a draft is saved when Shale closes**, even while a question is open. If that fails, its text goes to the recovery folder and is announced at the next start.
- **Mail: replacing a draft attachment by another file of the same name and size** now keeps the new file; the old one is moved to the local trash.
- Mail of a folder whose server numbering (UIDVALIDITY) changed no longer disappears from the lists and search when it is older than the three years that are fetched again or is no longer on the server: it stays, shown as not on the server, and no change is sent for it. Mail the new numbering has again shows once.
- An index restored from a backup no longer sends the actions that were queued when the backup was made: they are undone locally and the notice says how many, so you can check them.
- The local copy of a sent mail never overwrites an older file in the Sent folder; it takes the next free number.
- An autosave could land in the middle of a git sync's merge: the note was written, git then overwrote it with the merged version, and the text you had just typed was gone without a conflict copy. The sync and the window now take turns on one lock: the autosave skips a moment while git changes files, and every other save (save all, rename, move, trash, checkbox, fix link) waits for it for up to 30 seconds; if the sync is still busy after that, nothing is saved, your edits stay in the open note and a message says so. (SYNC-013)
- The troubleshooting key recording no longer writes anything typed or pasted into any password
  field (social accounts, the mail password prompt, calendar and mail settings): every password
  field is made one way, and a test refuses a new one made differently. When Shale starts with the
  recording on, a notice says where it is written and how to switch it off.
- A CalDAV or social-account password that the system keyring refuses (for example the unlock
  was cancelled) is no longer written to the settings file in plain text. It is kept for this run,
  Shale says it was not saved, and a password already in the file from an earlier version stays there
  until the keyring takes it. Without any keyring nothing changes (the owner-only settings file).
- A renewed mail sign-in (OAuth) that could not be saved in the keyring is kept for this run, saved
  at the next renewal, and reported once instead of silently needing a new sign-in after a restart.
- GnuPG's temp folders are no longer made directly in the shared temp folder: they are made new
  inside a private folder, and an existing path is never adopted.
- In Drafts and Outbox the message keys (`d`, `u`, `f`, `m`, …) no longer act on a message of the
  folder that was open before.
- **A mail server line longer than the limit is refused even when it arrives in one piece.** The
  length limit of an answer line was only checked while the end of the line had not arrived yet,
  so a line that came in one chunk could be longer than the limit (MAIL-024, MAIL-221).

- **Links written `HREF=` in capitals in an HTML mail are shown as links** (they lost their address in the plain-text view).
- **A CalDAV answer bigger than the allowed size looked like an empty answer.** When the server
  announced an answer over the limit, the request ended with status 200 and no content. It is
  now an error that says the answer is too big (SEC-001).
- **Encrypting to an expired or revoked key now says why.** GnuPG refuses such a recipient without
  a reason of its own; the message used to say only "GnuPG refused the key" and now says that the
  key cannot be used because it has expired, been revoked or been disabled (MAIL-193).
- **Dragging the end of an arrow away from its shape.** The end snapped back to the shape on every
  frame until the mouse button went up, so the arrow was detached but did not move. The end now
  follows the pointer.
- **A mail attachment that Shale refuses to open (a program, a script, a `.desktop` file) no
  longer leaves its copy in Shale's temporary folder.** The copy was made before the check and
  stayed behind when the check said no (SEC-006).
- **Windows: calendar events in a named time zone (`Europe/Berlin`, CalDAV `TZID`) keep their
  zone.** Windows has no zoneinfo files, so such events were shown in the computer's own zone.
  Shale now brings a time zone database of its own there (the crate `jiff-tzdb`, MIT or
  Unlicense) (CAL-018, #72).
- **Summer time far in the future is right.** Time zone files that list no changes after the rules
  became regular (Europe/Berlin: 1996; the bundled database, and the zoneinfo of some Linux
  distributions) kept the last offset for ever, so a winter day in 2030 had summer time; the rule
  at the end of such a file is now used for the later years (CAL-023, #72).
- **Thunderbird links.** A message that Thunderbird has deleted but not yet compacted out of its
  folder is no longer found by the subject or Message-ID search (a live copy next to it is), and when
  only such a deleted copy matches, the import says that the message was deleted in Thunderbird
  instead of “not found”. A
  position in a folder file must now start at a real header line or the mbox `From ` line
  (IMP-030).
- Deleting an event, or one date of a repeating event, from a local calendar whose `.ics` file was deleted outside Shale no longer writes the file again (a series without that date, or an empty calendar). Shale shows an error (“Could not delete the event from “Home”: the calendar file is gone”), writes nothing, and reloads the calendar, which is then shown empty. Saving a new or edited event still creates the file again (raba/shale#82).
- **Mail: a cancelled or wrong passphrase when signing is told in words.** GnuPG reports these
  for signing as a failure line, so the message said only "GnuPG could not do it"; it now says
  that the passphrase dialog was cancelled or the passphrase was not accepted (MAIL-171).
- **Mail: a revoked S/MIME certificate is shown as revoked.** gpgsm reports a signature of a
  revoked certificate as a good one followed by "not trusted, certificate revoked"; the status
  strip now says the certificate was revoked instead of a valid signature (MAIL-174) (#83).
- **The command line no longer guesses.** An unknown option, an option without its value or a
  second folder name stops the start with an error and the usage line (exit code 2). An option an
  older build did not know used to be skipped, and its value was opened as a new vault. That is
  how a stray `1360x840/` folder got into the repository; it is removed.
  `docs/screenshots/regen.sh` now refuses a `shale` binary that is older than the sources (#85).
- **Settings: the Done button stays in view.** In a window shorter than the Settings tab (most of
  all the long Keybindings list) the Done button was below the visible dialog and only came into
  view after scrolling to the end. The tab's content now scrolls inside the dialog and Done stays
  at the bottom (NAV-027, #87).
- Spec: the keybinding row KEY-003 (a binding is a chord or a sequence, stored as text), lost in a
  merge, is back, and a new test fails when a `// spec:` comment in the code names a row that does
  not exist (DEV-014, raba/shale#95).
- **Mail keys work after Tab.** Once Tab had moved the keyboard focus to a row of the Drafts or
  Outbox list, the mail keys (`e`, `j`, `k`, Enter, …) did nothing until you pressed Esc or
  clicked. They now work while a row has the focus; text fields still keep their keys. The rows
  of the message list show the same accent frame as the Drafts and Outbox rows when Tab moves
  there (MAIL-073, MAIL-224) (#96).
- CalDAV redirects work with older curl (7.x, e.g. Debian 12), which put the user name and password into the final address; Shale refused its own redirect and would have kept the password in the address (raba/shale#104).
- Mail sync no longer stops with "a message body that is not a string" when the server's answer arrives in pieces that split a line end between `\r` and `\n` (raba/shale#108).
- "Test connection" in the mail account settings signs in to SMTP with the password typed in the dialog, so an account that is not saved yet no longer fails with "no password for this account" (raba/shale#108).
- Starting Shale again right after quitting it could, very rarely, wrongly say "Shale is already running" when the first one had just started a program such as git or gpg; the second start now tries for 200 ms before it says so (raba/shale#113).
- **The mail view in a narrow window.** Every pane of the mail view now keeps a least width
  (folders 170, message list 220, message 280 points). When the window is too narrow for the three,
  the panes no longer squeeze the message until it is gone: they scroll sideways, and the pane that
  has the keyboard (Ctrl+H / Ctrl+L, a click) is scrolled into view. The detail of an Outbox mail
  scrolls and its buttons wrap, so "Discard" and the others can always be reached
  (MAIL-225, MAIL-226; raba/shale#122).
- **No empty band above the buttons of the Import dialog.** A dialog that fits the window was left
  about 26 px taller than its content, because the window kept the larger size of the first
  frames, when a wrapping line was still measured taller. It now takes the size it measured last
  (NAV-028).
- **A dialog gets shorter with its content.** A dialog you have not resized by hand now follows its
  content both ways: when the content gets shorter, or when the dialog is opened again with less
  content than it had when it was last open (for example Import with one item after one with many
  lines), it no longer keeps its old height with an empty band above the buttons. A dialog that
  grew with its content ends exactly like a new one. A dialog you resized yourself keeps your size
  (NAV-013, NAV-028; raba/shale#142).
- **The mail settings no longer jump when GnuPG's answer arrives.** With the Encryption section
  open, the line "Looking…" was replaced by several lines and, in a window taller than the dialog
  needed, the dialog grew and the controls of the section (or the buttons Done and Cancel) moved
  under the pointer. The room for the answer is now kept from the start (MAIL-191;
  raba/shale#151).
- **The "⋯" menu of the Feeds toolbar no longer jumps while a check runs.** The spinner of a
  running check was put in front of it and pushed it 26 px to the right; it now shows inside the
  "Check now" button, which keeps its size (SOC-041; raba/shale#153).
- The right sidebar can be resized again: it kept snapping back to its first width when the separator was let go (raba/shale#155).
- Mail: rows of the Drafts and Outbox lists that are not selected keep their margin instead of touching the edge of the pane with their first letter cut off (raba/shale#166). The two outbox mails of the screenshot demo mail are always listed in the same order.
- **The mail settings no longer jump when a long GnuPG answer arrives.** The answer in the
  Encryption section has a fixed height of five lines; with more secret keys or certificates than
  fit, the list scrolls inside it instead of making the dialog grow and moving the controls and the
  buttons Done and Cancel (MAIL-191; raba/shale#169).
- **"Save to vault" and "Load more in between" no longer push the buttons next to them aside.**
  Their spinner now shows inside the button, like "Check now" (SOC-042; raba/shale#194).
- Note suggestions: a click on an entry of the `[[` list picks it, like Enter or Tab; before, the list closed under the mouse and nothing was entered (raba/shale#200).
- Pasting a picture copied in a browser saves the picture and links it, instead of pasting its `<img src="image.png">` tag (raba/shale#219).
- **Imported and pasted audio and video now show their player.** Import, drops and pasting wrote a
  plain `[[…]]` link for sound and video files, which stays a link; they now write `![[…]]`.
  Notes that already have the plain link: add a `!` in front (#220).
- **Clicking the play button of a player in the editor no longer moves the text cursor.** The
  button, the mute button and the artwork now take the press like the seek bar and the volume
  slider did (#221).
- **A flag removed in Thunderbird (or any other mail program) shows up in Shale within seconds.**
  Changes made elsewhere only arrived with the next sync, up to ten minutes later. While the mail
  view is open, the folder you have open and each account's Inbox are now watched over a read-only
  IMAP IDLE connection, and a quick check runs when the server reports a change (a server without
  IDLE is asked once a minute). The check never runs beside a full sync of the same account
  (MAIL-230…MAIL-234; raba/shale#227).
- A toolbar colour, highlight, bold etc. on a selection that spans table cells is applied to
  each cell on its own, so it shows in the grid instead of leaving broken tags across the `|`
  (EDIT-027, EDIT-029).
- **The outline shows formatted headings as the editor does.** A heading such as
  `# <mark style="background: #FFFF00;">Audio</mark>` no longer shows its HTML or markdown in the
  Outline panel: bold, italic, code, strike, highlights (with their colour) and links show as
  formatted text, without the markup (LINK-013; raba/shale#248).
- **Settings file tab: text with an error is no longer lost on quit.** If the tab still has text
  that is not saved when Shale closes (it has an error, or the file was changed by another program
  meanwhile), the text goes to the recovery folder as `settings/config.toml` and the next start
  tells you; the settings file itself is left as it is. Valid text typed just before quitting is
  saved first (raba/shale#254, CFG-015).
- **A very short video without sound no longer ends before its first picture.** The clock of a
  video now starts with its first picture, after the start and after each seek, so a slow ffmpeg
  (a loaded machine) cannot play a one-second clip to its end and rewind it before anything was
  shown (MEDIA-009, #259).
- **Links and coloured text in a system font.** With a chosen font such as Calibri, some letters of a
  link (or of any coloured text) were drawn white and out of place, and the underline did not fit:
  egui took the font's embedded black-and-white bitmaps for colour glyphs. Shale now hides those
  bitmaps from fonts that have outlines, so every letter takes the colour of its text (CFG-038, #265).
- **The mail status bar no longer counts changes that can never be sent.** Changes queued for an
  account that is gone, or whose server or user you changed (which makes it a new account), were
  counted as "waiting for the server" although they will never be sent. They are now shown apart
  as "N queued changes belong to removed accounts (not sent)"; they are kept, not deleted
  (MAIL-207, MAIL-208).
- **Mail: a sent copy that was left behind by a restored backup is no longer forgotten.** The restore dropped the waiting upload of the copy but kept the mail, so it was never uploaded and nobody noticed. It is now found and offered for upload again (MAIL-205, MAIL-210).
- **A draft whose attachment was lost when Shale stopped says so, and nothing is sent without it.** When you replace an attachment of a draft, Shale puts the old file in the trash and then writes the new one. If Shale stopped between the two, the draft kept pointing at a file that was gone and nobody told you. Now Shale first writes down what it is about to do. When you open the draft again it tells you which attachment is missing and where the previous version is in the trash, and lets you restore it, remove the attachment, or dismiss the message. A mail in the outbox whose attachment file is missing is not sent: it is marked as not sent and says which attachment is missing. The copy in the trash is never deleted. (MAIL-215 to MAIL-219)
- **Windows: pasting copied files into a note.** A `file:///C:/…` address (what a file manager
  copies) is now understood, so pasted pictures, PDFs and media files are saved to the media
  folder instead of being pasted as text.

### Tests

- A bare-key action (Mail view `s`) does not fire with Shift or Alt held; this already held since
  the exact-modifier match and is now pinned by a test (KEY-010).
- `o` (open attachment) with a pretend program (nothing starts), `]` across several
  attachments and `z` (undo send) are tested (MAIL-065).

### Internal

- **Flaky and platform-specific tests fixed**: tests that failed now and then under load, in
  parallel runs or because of the shared temp folder, and tests that failed only on Windows (#31, #43, #64, #66, #67, #68, #80, #127, #136, #171, #218, #231, #232, #247, #252, #260,
  #261).
- **CI and build fixes**: GitHub actions pinned off Node 20, each commit built once with a kept
  Cargo cache, dead code that only showed in the macOS and Windows clippy removed, and no
  jobserver warning from the local linker shim (#41, #65, #123, #204, #216, #246).
- **Code and test tooling**: a dialog's content closure returns nothing unused, dialog placement is
  tested at any window height, and a setting that was missing from the user guide's settings
  reference was added (#141, #170, #212).
- The changelog names every issue closed since 0.3.0, as the release check asks (#276).
- **Screenshots**: a Markdown table in the editor, the reading view and the PDF export dialog, and
  smaller lossless PNGs (#222, #230).
- **Tests for the Thunderbird reader, the start dialog and starting up.** A generator builds
  throwaway Thunderbird profiles (accounts, mbox and maildir folders, nested and non-ASCII folder
  names, indexes, deleted and large messages, broken files) for the tests of the import from
  Thunderbird (IMP-018). The start dialog (open or create a vault, recent vaults) is driven
  headlessly (VAULT-021, VAULT-022), the single-instance check and the command line are tested
  (DATA-012, DEV-013). No change in behaviour.
- **CI runs for tags only; tags and releases are made by script** (ADR-0018, #274). The Forgejo
  CI no longer runs for every push to `main`: `scripts/land.sh` checks every change before it
  lands, and now also moves the local `main` forward. `scripts/make-tag.sh` makes a pre-release
  check tag (`v0.4.0-rc.1`), which Forgejo checks once before `main` goes to GitHub.
  `scripts/make-release.sh` makes a release: it bumps the version, writes the changelog section
  and tags. Both stop with "the changelog needs an update first" when the changelog has
  nothing new. GitHub builds release tags only, never check tags.
- **Flaky mail watch test fixed**: the pretend IMAP server now tells a client, as soon as its next
  `IDLE` starts, what changed while it was busy between two `IDLE`s, as a real server does; a new
  mail added in that gap was lost and the test timed out (#286).
- **The TeX test image builds on arm64**: it installs the TeX Live binaries of the machine it is
  built on (`aarch64-linux` on the arm64 Forgejo runner, `x86_64-linux` elsewhere) instead of
  always the x86_64 ones (#289).

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

# Changelog

All notable changes to Shale are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (before 1.0, new features raise the middle number and
fixes the last one).

The release page of each version on GitHub shows the AI notice (`.github/ai-notice.md`) and then
its section from this file.

## [Unreleased]

### Added

- Drawings: a selection of several shapes now has a resize box around it. Dragging a corner scales every shape, line, arrow, scribble and free text in the selection from the opposite corner (#99).
- A drawing that embeds a picture from outside the vault can copy it in: **Copy outside media into the vault** in the ⋯ menu of the drawing copies the pictures into the media folder and points the drawing's `## Embedded Files` entries at the copies. The drawing keeps its format, and a picture that is missing or already in the vault is left alone (#512).
- Settings → Appearance → "Properties in the editor" = Card now draws the typed card (mail header, event, Word page chip, rows, tags) of the reading view in place of the folded properties line; a click on it opens the properties for editing (#556).
- Feeds: a click on a post's picture opens it big in a viewer over the tab, fitted to the window, with the picture's description. Left / Right (or the buttons) step through the post's pictures, the mouse wheel and `+` `-` zoom, `0` switches between 100% and Fit, Esc closes. A spinner shows while the picture loads and a message when it cannot be loaded (#559).
- Feeds: a click on a post's reply count (or the new **Show thread** button) opens its conversation in the same tab: what the post replies to above it, the post highlighted, and the replies below as a tree. It loads in the background with a spinner and a Retry on errors, the posts are drawn like in the feed (pictures with the viewer, content warnings, Save to vault), and Back returns to the feed at the same place. Shift+click or a middle click opens it in a new tab; the posts are added to the post search; on a pinned tab it opens in a new tab (#560).
- Feeds: you can write now. Every post has **Reply**, **Boost** / **Repost**, **Favourite** / **Like** and **Quote** buttons (in the feed and in a conversation), and the toolbar has **New post**. Boost and Favourite toggle, show whether you did it and change the counts when the server said yes (a spinner inside the button, nothing moves). The compose box counts against the limit of your network, offers a content warning and visibility on Mastodon, up to four pictures with alt text (Bluesky pictures over 1 MB are scaled down), real quotes (Bluesky; Mastodon 4.5 and later, otherwise the post's address is added to the text), makes Bluesky links, mentions and hashtags clickable, and always asks "Post this?" before sending (Ctrl+Enter sends). A sent post waits behind the "new posts" button, a reply joins the open conversation, and a failed send keeps your text (#561).
- Feeds: a click on the preview of a post's video or GIF now plays it in the viewer instead of opening the browser. The file is downloaded in the background into the picture cache (https only, no login sent, at most 64 MB; a Bluesky video as its real file), with a spinner and an error message. The media player's card gives play / pause (also Space), seek and volume; a GIF plays muted and looped. Left / Right step across pictures and videos, and stepping away or closing stops the playback. Without ffmpeg the viewer shows the preview picture and says so, and Open in browser stays (#564).
- Mail: "Delete local copy…" (button, row menu, key Shift+D) removes Shale's own copy of mails that were deleted on the server, after a confirmation, several at once. Mail that is still on the server cannot be removed this way; the server and imported notes are not touched, and the files go to the local `.trash` (#566).
- Calendar: an event can be imported as a note with an **Import** button in its details popup and an **Import** entry in its right-click menu, besides dragging it out of the calendar; both open the same Import dialog (#568).
- Ctrl+Tab and Ctrl+Shift+Tab switch to the next / previous tab of the focused pane, in the order of the tab bar, after the last comes the first; they are a second default key of the two actions and can be rebound in Settings → Keybindings (macOS keeps Cmd+Opt+← / Cmd+Opt+→, Cmd+Tab is the system's) (#571).
- Layout: Files, Search and Tags can be placed in the right sidebar as well, and back. Drag the icon from the ribbon to a right menubar or to the right sidebar's content (or the other way); on release Shale asks **Move or copy?**: move takes the view from the side it came from, copy shows it on both sides with the same state (one file tree, one search). Right-click the icon for **Move to / Copy to the right (left) sidebar** without dragging. The placement is kept in the settings and old layouts load as they were; when the left sidebar's view has moved it shows the next one, and Ctrl+Shift+F shows Search where it is (#573).
- The editor completes tags while you type: `#pr` (and a word in the `tags:` property of the front matter) lists the vault's tags that match, nested tags included, best match first; `#prj` finds `projects/shale`. Up/Down choose, Enter or Tab pick, Esc closes. Nothing opens in code, in a link, in a URL or on a heading (#574).
- Import: a follow-up mail continues its thread. The Import dialog finds notes of the same conversation by the reply headers of the mail (`In-Reply-To` and `References`, never the subject), offers "Continues [[note]]" with a tick box, puts the new note in the folder of that note, and links the notes with `previous` and `next` properties in date order (a `thread-id` ties them together; only the new note and at most two neighbours change, and only their `previous` / `next` lines). An older mail asks "Keep the thread in date order?" (Update or Leave the chain). The new note and the neighbour lines are one entry of the file history. 
- A new **Thread** view in the right sidebar lists the mails of the open note's thread, oldest first. Ctrl+Shift+PageUp and Ctrl+Shift+PageDown step to the previous and next mail of the thread, Ctrl+Shift+T opens the view and gives it the keyboard (#575).
- Saving a post or thread from the feeds continues its conversation, like a mail: the post note gets `in-reply-to` and `thread-id` (the root of the conversation, in the same network) and the Import dialog offers "Continues [[note]]" for a saved post it replies to or that it is a reply to, with `previous` / `next` links in time order and the same question for an older post. The Thread view lists and steps through the posts of a conversation (#575).
- A book's title page can show a picture: the folder note's `logo:` property (a vault path, a file name or `![[logo.png]]`) is drawn centred above the title, and a missing or unreadable picture is skipped with a warning. The PDF user guide shows Shale's logo this way (#577).
- A table of contents is now followed by a page break in all paged exports (built-in PDF, LaTeX, Word and folder books), so the text starts on a new page. The export dialogs have a box **Page break after the table of contents** (on by default), and a note can set it with `print-toc-page-break: false` (#578).
- A sound attached to a mail (mp3, ogg, oga, opus, m4a, aac, wav, flac) has a player in the reading pane, the same one as in notes; it plays from a private temporary copy and never by itself (#579).
- The vault label at the bottom left of the window now opens a list of the vaults Shale knows (#583). The open vault is marked; **Open…** and **Create…** add a folder (Create only takes an empty folder, hidden files such as `.DS_Store` do not count, and offers sample notes; a folder with content can be opened instead); a click on a vault opens it; arrows, Enter and Esc work. A right-click on a vault renames it (an empty name is refused). A vault whose folder is gone is greyed out in the list and on the start screen with **Locate…** and **Forget**, and Shale never makes that folder again on its own. A folder inside another known vault, or around one, is allowed and warned about once. Closing a vault from the list and the "Switch vault" command follow in #584.
- A vault can be closed (#584). Right-click a vault in the list at the bottom left and choose **Close**, or choose **Close current vault** at the end of the list: the vault is taken off the list and its notes stay where they are. A question asks whether to remove Shale's own folders too, `.shale/` (the vault's settings and state) and `.trash/` (the notes you deleted, with a warning that they are gone for good); both are off by default, and notes, attachments and a `.git` folder are never touched. Closing the open vault saves your edits, waits for a running sync, and shows the start screen with the list open. The quick switcher has a command "Switch vault" that shows the same list.
- A folder that has its own `.git` now asks, when you open it or add it as a vault, whether to use git sync with it: **Use git sync** keeps auto sync on, **Don't sync** turns it off for that vault. Nothing is synced before you answer; Esc asks again next time (#585).
- Feeds: the compose box has a **Choose pictures…** button that opens the system's file dialog (the desktop portal on Linux), takes picture files dropped on it (they are not imported into the vault) and pastes a picture from the clipboard with Ctrl+V; typing a path still works, also when no file dialog is available (#586).
- Feeds: the post quoted inside a card has buttons of its own (Reply, Boost / Repost, Favourite / Like, Quote, Show thread, Save to vault, Open in browser, Copy link) that act on the quoted post, in the feed and in a conversation. The state of a quoted Bluesky post (liked, reposted, counts) is read along with the page, and a Like or Repost on a post whose state is not known first reads it, so nothing is liked twice (#588).
- Feeds, compose box: Mastodon **polls** (2 up to the server's limit of options, a duration, several choices, hidden totals; not with pictures or a video), **scheduled posts** (**Schedule…** with a local date and time at least 5 minutes ahead; **Scheduled posts…** in the box and the feed's menu lists them and cancels one), a **language** for the post (new setting **Post language** in Settings → Feeds; Mastodon `language`, Bluesky `langs`) and **one video** per post (Mastodon through the media upload, Bluesky through its video service with a service token; at most 100 MB). Polls of Mastodon posts are shown on the cards with bars and percentages and can be voted in, with a spinner in the Vote button (#589).
- Feeds: **Notifications** is a feed in the picker of every Mastodon and Bluesky account. A row per notification (what happened, who did it, how long ago, a dot while unread; likes and boosts of one post come together as "Ada, Bo and 3 others …") above the post it is about, drawn as a normal card with all its buttons; a follow shows the account and Open in browser. It is checked like the other feeds, new ones wait behind "N new notifications", the tab title counts the unread ones, and showing the list tells the server that you read them (Mastodon's marker, Bluesky's "seen"). The posts are in the search index (#590).
- Feeds: **Your posts** is a feed of your own posts, replies and boosts; a post you send from Shale waits there too (#590).
- The search (the sidebar and `Space f g`) reads one query language: `AND`, `OR` and `NOT` in upper case, `-word`, parentheses, and the fields `#tag`, `status:doing` (any property of the front matter), `path:Folder/`, `due:<+7d`, `due:overdue`, `due:2026-12-01`, `links:[[Note]]`, `blocked` and `blocking`. Fields select notes and the text terms find the lines in them. Mail and posts take only words and their own fields, and the search says which source it skipped ("Mail: `status:` not supported") (#597).
- A query is checked before it runs: a syntax error (an open parenthesis, a dangling `AND`, `due:soon`, `priority:urgent`, a query over 500 characters, 32 terms, 8 levels or 8 `*` in a word) is underlined with its reason, nothing runs and the last list stays (#597).
- A search has a work budget. A pattern such as `*a*a*a*a*b` over a long line used to take time quadratic in the line; when the budget is used up the search ends with a partial list marked "Stopped early, narrow the query" (#597).
- The search has a **filter form**: a ⚙ button next to the search box of the sidebar and of the picker opens rows for tag, property, due date, blocked, blocking, folder, link and text (each with a "not" switch), joined by **All of** or **Any of**. The form writes the query into the box and the list follows; what you type in the box fills the form. **Advanced** shows the text with a live count of matches, and is remembered (`search.advanced`). A query that is more than one level of AND and OR is shown as text and left as it is. Ctrl+Shift+G opens the form with the keyboard. The same form has sort rows for the lanes of the boards that follow (#599).
- Every Markdown note Shale saves now has `created:` (the time of its first save, never changed) and `modified:` (the time of the last save that changed the text, not one that changed only the properties) in its front matter, as ISO 8601 times with the time zone. A note may have `priority:` with `highest`, `high`, `medium`, `low` or `lowest`; it shows in the folded properties line. The file list sorts by Created and Modified use these dates, with the `YYMMDD_` name prefix and the file time as fallbacks (#600).
- Kanban boards: a note with `type: board` is a board whose lanes (`##` headings with `query:`, `sort:`, `drop:`, `done:` lines and pinned `[[links]]`) fill themselves from a query on the background search worker. **Space b** (the definable action "Show the boards") opens the boards in a tab with a strip of every board of the vault and **+ New board**; lanes are added, renamed, moved and deleted in the view, which rewrites the file and keeps every line it does not know, and a double click on a lane's heading opens the filter and sort form of the lane. Cards show the due date, status, priority and tags (`card-shows:`), a drawing shows its picture; a click selects a card and opens a peek, a small editor of its note with a Priority control, Ctrl+Click or Enter opens the note in a tab. A lane with a wrong query shows the reason in its heading (#601).
- Boards: drag a card onto a lane. The lane's `drop:` property, or the one its simple query says (`status:doing`), is written into the note's front matter without moving `modified:`; a lane without one pins the card where it was dropped, and the order of the `[[links]]` is the manual order. Esc cancels (#602).
- Boards: dependencies between cards. `blocked-by: [[A]]` in a note makes its card show a "blocked" chip until A is done (`status: done`, or a card in a lane marked `done: true`); selecting a card draws arrows to its blockers and to the cards that wait for it; `sort: dependencies` puts a lane in dependency order with step numbers (step 3a and 3b for cards that can run side by side) and marks a cycle as "circular"; `start:` is a chip of `card-shows:`; the menu next to "+ Add lane" has the preset "My dependency chain" (#603).
- A board exports as one PDF: **Export PDF…** in the board view (or **Export to PDF** of a board note) writes the board on landscape pages, the lanes as columns with their cards, then the notes of the cards through the book export with a contents page, bookmarks and links from the cards (#604).
- A board can be exported through a LaTeX template too: the export dialog of a board has the **LaTeX template** engine, which writes the board on landscape pages (real landscape pages, lanes as columns of cards that LaTeX fills page by page), then the notes of the cards as parts (lanes) and chapters (cards) of a thesis or report, with the contents (#605).
- Layout: Mail, Calendar and Feeds can be placed in the right sidebar, like Files, Search and Tags: drag the icon to a right menubar or to the right sidebar's content and answer **Move or copy?**, or right-click it for **Move to / Copy to the right sidebar**. Each pane is a compact companion of its tab, not the tab squeezed into a narrow column, and keeps its own state (account, folder, month, feed, scroll), separate from the tab, while the data and the background work are shared. Copy keeps the ribbon icon, which still opens the tab; old layouts load as they were (#610).
  - **Mail**: the accounts, the folders with their unread counts and the message list with the unread state. A click on a message opens it in the Mail tab (opening the tab when there is none).
  - **Calendar** (not to be confused with the older Calendar panel): a small month with ‹ › and Today, today marked and a dot under the days with events, and the agenda of the next 14 days from the chosen day, all-day events first, each with its time and title. A click on a day moves the agenda there; a click on an event opens it in the Calendar tab with its details.
  - **Feeds**: the accounts, a feed picker (the home timeline first) and the posts as small rows with the author, the age, the first two lines of text and the number of pictures. A click on a post opens its conversation in a Feeds tab (the active one, else one of that feed, else a new tab). The pane fetches the first page of the feed it shows, also with no Feeds tab open; older posts and the check for new posts stay in the tab. It says so when there is no account or no posts yet and while the feeds load.
- Space Space checks or unchecks the task on the cursor's line (editor in vim normal mode), or the task under the pointer in the reading view and the task list; a line without a task says so. It is the definable action "Check or uncheck the task" (#611).
- Boards: with a card selected, Alt+Shift+Right / Alt+Shift+Left (the definable actions "Boards: move the selected card to the next / previous lane", Main views) move it to the next or previous lane like a drop below that lane's pinned cards, and a card dragged near the top or bottom edge of a lane, or the left or right edge of the board, scrolls the lane or the board so a drop further away is possible (#627).
- For developers: `cargo run --example loc_report` writes a Markdown report of the project's lines of code: production code, test code, comments and blank lines per feature (top-level module of `src/`) and per featureset (`tools/loc-report/featuresets.toml`), with the test share, the integration tests and the shell scripts (DEV-044, #628). Every landing commits a fresh copy as `docs/loc-report.md` (DEV-045).
- A vault can be written as a Hugo website: `shale export-site <vault> <out>` (with `--dry-run` to list what it would write) writes the notes the vault's publishing rules choose as a complete Hugo site, with the Shale theme, backlinks, tags, typeset math and a search box for Pagefind. Publishing is off until `publish.enabled` is set in the vault's `.shale/settings.json`; nothing of a note that is not chosen, no hidden file, conflict copy or mail original is ever written, and the vault is never changed (#630).
- A vault can be published from Shale: **Settings → Publishing** has the vault's switch, the rules for the notes (with the count of the chosen notes and a list), the address and branch of a repository of your own and a **Publish** button that runs in the background with its progress and Cancel. A publish writes the site into a repository that Shale keeps in its own data folder and pushes **one snapshot with no parent**, so a note you stop publishing is gone from the branch and its history; audio and video go to Git LFS and are not sent again when the server has them. It signs in as git sync does (a token from the keyring or an SSH key) and says why a sign-in failed; it refuses an address with a password or the address the vault syncs with, and asks before it replaces a branch that another vault published to. The page shows from which computer and when the target was published last. `shale export-site --publish <vault> [--take-over]` does the same without a window. The user guide has a recipe for a Forgejo Actions workflow and a post-receive hook that build the site with Hugo and Pagefind on your server (#630).
- A vault with no chosen note (or with publishing off) now writes and publishes a site with one generated hello-world page, with the time of the export and nothing of the vault, so that you can see that publishing and your server's build work (#630).
- Phase 2 of the website export: an embedded sound, video or PDF shows a player in the page (with the width of a picture and a link to the file inside); an embedded drawing (`.excalidraw`, `.excalidraw.md`, `.canvas`) is drawn to an SVG picture with the links to published notes working and the other links left as text; the site has a graph page of the published notes (`/graph.html`, a small script of the theme, nothing loaded from another site); notes of type `email`, `event`, `post` and `person` show a header with their own properties, any other type the plain page (#630).
- Phase 3 of the website export: a published board note shows its lanes and cards as they were when you published, read-only, with the lanes scrolling sideways and the chips the board shows (due, status, priority, tags). A card of a note that is not published is left out without a trace (no title, no count), and the lanes' queries, pins and tags are not written; a lane that cannot be worked out shows a note and is a warning in the publish summary (#630).
- Phase 4 of the website export: a site map page (`/sitemap.html`) lists the published notes as a tree by folder and is linked from every page next to the graph. A new optional setting, **Address of the site** (Settings → Publishing, `publish.site_address`), gives the site a `sitemap.xml` of the published pages (no dates, no tag pages) and a `robots.txt`, and makes the links of a site that is served below a path such as `https://host/wiki/` work; the links inside a drawing's picture are now relative to it (#630).
- The filter form suggests what the vault knows: the field of "tag is" lists the tags, "property is" the names of the properties and, for the name chosen, the values seen with it. An empty field lists everything alphabetically, typing narrows the list (what starts with the letters first), and ↓/↑, Enter, Tab or a click take an entry; Esc closes only the list. It works in the lane settings, the picker and the sidebar search (#631).
- Git sync tells a failed sign-in in plain words with a short fix, and for most cases a link **How to fix this** into the user guide (the new note "Git sync sign-in problems"), in the wizard, under **Sync now** in Settings → Sync and in the toast: an SSH key with a passphrase that your ssh-agent does not hold ("locked"), no ssh-agent running, a password where the server wants a token (two-factor login), a login page of a proxy in front of the server, and a changed host key. A key of your own with a passphrase works through your ssh-agent; Shale never asks for or keeps a passphrase. The Forgejo test stack now has an SSH server, and SSH with and without a passphrase and the HTTPS token are tested against it (#635).
- The LaTeX export compiles in several passes: LuaLaTeX, then biber when the document has citations, then LuaLaTeX again until references and numbers are stable (at most 4 runs). A citation key missing from the bibliography is a warning, and a TeX without biber gives the PDF without a bibliography and says so (#644).
- Thesis writing, part 2 (#645, #642): a note can name its literature in the property `bibliography:` (a path in the vault, a file name or `[[refs.bib]]`; a list for several), and Settings → Vault has a **Bibliography file** for notes without the property. Shale reads the BibLaTeX or BibTeX file (as Zotero with Better BibTeX, JabRef or Citavi export it) in the background into an index of the keys with author, year, title and type, skips and counts an entry that cannot be read, reads the file again when it changes, and says in Settings what the file holds and which keys the open note cites that the file lacks. One scanner for citations (`[@key, p. 12]`, `@key`), labels (`{#fig:results}`) and references (`@fig:results`) is built and tested; nothing is drawn in the editor yet.
- Thesis writing, part 3 (#646, #642): citations, labels and references work in the editor and the reading view. After `[@`, `; @` or `@` the editor lists the keys of the bibliography with authors, year and title, and after `@fig:`, `@tbl:`, `@eq:` or `@sec:` the labels of the document with their number and caption; hovering a key shows its entry, hovering a reference its number and caption (Ctrl+click keeps it open); a key the bibliography lacks, a reference to a label that does not exist and a label used twice are drawn red like a broken link. The reading view writes a citation as the chosen style does (`citation-style:` numeric, authoryear, alphabetic, ieee or apa, formatted with the CSL engine `hayagriva`), a reference as its word and number in the language of the note (`language:` de or en: "Abbildung 3", "Figure 3"), numbers figures, tables and equations with their captions (above or below as `figure-caption:` and `table-caption:` say, continuous or per chapter as `float-numbering:` says, also across the chapters of a book) and draws the list of references at the end or at `<!-- shale: bibliography -->`. The release build is about 6 MB larger for the style and locale files that come with `hayagriva`.
- The export dialog of a note now has a **Fonts** row: when the note (`print-font`, `print-font-sans`, `print-font-mono`) or the vault's settings (`print_font`, `print_font_sans`, `print_font_mono` in `.shale/settings.json`) name a font, it says which one the export would use and where it was found (the vault's `fonts/` folder or an installed font), and warns when a font is missing or its licence does not allow embedding it. The fonts are looked up in the background (#658). The built-in PDF export uses them; the LaTeX export follows.
- For developers: the lines-of-code report starts with a chart of the code and test code per `v*` tag, counted on each tag's tree (`git archive`, no checkout): `docs/loc-history.svg` (hovering a tag in the SVG shows its date and counts) with the numbers in a table in `docs/loc-report.md`, the counts kept per tag in `docs/loc-history.tsv`. `--no-history` leaves it out. Every landing commits the three files together (DEV-046, #669).
- A note, a drawing or a folder can be dragged from the file list onto a board. On a lane a note is dropped like a card (the lane's property is written, or the note is pinned), a folder pins its notes and drawings there, and a folder dropped on the empty board or on **+ Add lane** makes a lane with `query: path:Folder/`. The lane is marked and a text says what the drop does; Esc cancels (#674).

### Changed

- A long code block (20 lines or more) is now coloured on a background thread: it shows as plain code for a moment and gets its colours when they are ready, without a change of its size, so a note with long code blocks no longer stalls when you scroll to them or type in them. Exports are still coloured at once (#245).
- The user guide shows the Vault and Editor tabs of Settings and the settings file in a tab of their own (#255).
- The font picker of Settings → Fonts draws each entry in its own font. The fonts of the entries on screen load in the background once you stop scrolling (until then the name shows in Shale's font), at most 64 stay loaded, and a symbol font shows its name with a sample (#273).
- The file list is much faster in a big vault: with 5000 notes and every folder open a frame took about 30 ms and now takes under 1 ms. The list is worked out once instead of every frame and only the rows in view are drawn; the keys, drag and drop, the context menus and scrolling to the open note work as before (#398).
- A vault is now read in the background: at start and when you open or create one, the start screen shows a spinner and "Opening …" with a Cancel button, so a vault on a slow network share or an online-only cloud folder no longer freezes the window. The window with your notes appears when the vault has arrived (#513).
- A picture that is still loading now has its size from the first frame: its file's header is read in the background, so the placeholder in a note (and the room of a picture inside a line) is as big as the picture and the text below no longer jumps when it arrives. Decoded pictures are kept up to 256 MiB of memory; the ones shown longest ago are let go first and decoded again when needed (#515).
- The PDF export of a note or drawing now runs in the background like the others: a note with many pictures no longer freezes the window, and Cancel stops the export before the next picture without leaving a half-written file. The PDF export through LaTeX, the export to Word (now with a spinner) and the conversion of Word files in the Import dialog are listed in the busy indicator with their own Cancel. Closing such a dialog stops its work (#524).
- More long actions say what they do and show in the busy indicator at the top right of the editor: reading the vault folder for outside changes (only when it takes more than 2 seconds), learning the words of your tags (also only after 2 seconds, with Cancel), reading dropped files and mails for the Import dialog, copying outside media into the vault (with Cancel; the files copied so far keep their new links), opening an outside file with another program, the Clear and the daily cleanup of a cache in Data usage, counting what Shale keeps, and the look for pandoc and TeX in Settings. Their buttons are off while they run (#525).
- Busy states for git and installs: the LFS downloads, the snapshots and reads of the file history, the steps of "Turn on sync" (signing in, making a key, creating the repository, moving `.git`), the install, update and removal of Shale's own git and of the TeX Live, the import of template sets, the test compiles and the check, download and install of an update now say what they do, cannot be started twice and show in the indicator at the top right of the editor. Installs, updates, imports and the check or download of an update have **Cancel** (what was downloaded is removed); quick housekeeping such as a snapshot of the history shows in the indicator only when it takes longer than two seconds (#526).
- The load of every CalDAV calendar, finding the calendars of an account, the requests of a feed (first page, next page, check for new posts, "Load more in between"), saving a post and signing in to a feed account are now listed background tasks with **Cancel**: "Syncing the calendar “Work”…", "Loading “Home” of @name…", "Saving the post of @name…" and so on. Cancel takes effect at the next request and leaves everything as it was: a calendar keeps its events, a feed keeps its list (and offers Retry), a stopped save writes nothing and a stopped sign-in keeps no token. The timer's calendar load and check for new posts show in the indicator only when they take longer than two seconds. The plain "syncing…" text and the spinners beside these actions are replaced by the task's text; closing the calendar or the accounts dialog stops its work (#527).
- Import: "Skip signatures" now cuts the signature text, too: the sender's own and the signature of each mail quoted below it (`-- ` at any quote depth, the signature blocks of Thunderbird, Gmail, Outlook and Apple Mail, a closing greeting with a short contact block, a disclaimer), and keeps the quoted text; what is not clearly a signature stays. A new option "Leave out quoted mails" (Settings → Import sets its start value, off by default) drops the quoted mails of a thread. The editor folds a signature inside a quote as well (#543).
- Mastodon sign-in now asks for `read write` and keeps the permissions it was given. An account that signed in with read-only access keeps reading; the first like, boost, reply or post says "Sign in again to allow writing" and opens that account's Edit sign-in (#561).
- ffmpeg and ffprobe now read files only with `-protocol_whitelist file`, so a clip cannot make them open other files or addresses named inside it.
- The cache "Social pictures" of the Data usage dialog also holds the videos and GIFs of posts, cleared and pruned with the pictures.
- A pinned tab is never replaced any more: an import, a followed link, a search result, the quick switcher, a file from the tree, and back and forward now open the note in a new tab and leave the pinned tab as it is (#565).
- The inline style keys and buttons (bold, italic, strikethrough, highlight, code) toggle: pressed again just before the closing marker they leave the span (the cursor moves behind it) instead of inserting a new pair, also nested; on an empty pair the second press removes it (#570).
- Space f g (search notes, mail and posts) and Space f f (find a file) now work in every main view: the editor, the reading view, mail, calendar, Feeds, tasks and the graph, also without vim-style editing. They do nothing while you type in a text field, under a dialog or in a drawing. They are the new scope "Main views" in Settings → Keybindings. In the mail view Space is a leader too, so marking a message moved from Space to **x** (changeable); the other mail keys stay (#580).
- Shale now keeps a list of the vaults it knows in the settings file (`vaults`, each with a name and a path) instead of the list of the eight most recent ones: there is no limit, a folder is never listed twice (symbolic links and a trailing slash are resolved), and a vault from `--vault` or a second start is added once it has loaded. The old list is converted at the first start. The start screen lists the vaults by name, and the window title shows the name of the open vault (#582).
- Switching to another vault (and closing one) now waits for a running sync, and drops everything of the first vault: tabs and panes, search and its results, the task filter, the file list's open folders, backlinks and the thread overview, the file history, the sync's last result and the question about a repository, the rescan and the jobs of the old vault. Mail, the calendar and the global settings stay.
- Feeds: the options of a Mastodon poll are no longer added to the post's text as a list; they are drawn as a poll (#589).
- A word that is an operator or a field in the new language (`AND`, `OR`, `NOT`, a `-word`, `#tag`, `key:value`, `blocked`, `blocking`) is no longer plain text: put it in quotes (`"AND"`) to search for it. A run of plain words is still one phrase, so "salt and pepper" stays a text search, and a pattern of `*` and `?` alone is now a syntax error (#597).
- Searching (the `Space f g` picker and the sidebar search) now runs on a background thread, so typing never waits for a search of a large vault: the last list stays with a small spinner until the new one arrives, and a newer query gives up the older one (#598).
- Rebuilding the mail index (Settings, Data usage, Mail index, Rebuild) is now a listed background task, "Rebuilding the mail index…": the indicator shows it, the mail view shows it with a spinner, and **Cancel** stops it between two folders. The rebuild works folder by folder, so a stopped rebuild leaves every folder either built again or exactly as it was, never empty, and the index stays sound (#606).
- The sync of each mail account is now a listed background task, "Syncing ‹account›…": the indicator and the mail view show every account that is syncing with a spinner and its own **Cancel**. Cancel stops that account between two folders or two batches of messages, so no message file or index write is left half done; the view says the sync was stopped and the next sync goes on normally. Accounts sync side by side, and cancelling one leaves the others running (#607).
- Sending mail is now a listed background task per account, "Sending mail from ‹account›…": the indicator and the mail view show it with a spinner and **Cancel** while the account's mails go out, and a failure ends it with the server's words in the view and the Outbox. Cancel stops before the next mail is taken from the Outbox; a mail that is already on its way is finished, because once its data has gone to the server nothing can take it back, and the mails after it wait in the Outbox until you queue, retry or send one again (#608).
- The browser sign-in (OAuth2) of a mail account is now a listed background task, "Signing in ‹account› in the browser…": the indicator, the mail view and the account dialog show it with **Cancel**. Cancel, or closing the account dialog, stops waiting for the browser at once and frees the local address the browser answers to. A sign-in is kept whole or not at all, so a token that arrives just as you cancel is dropped, and a second sign-in of the same account does not start while one waits (#609).
- A search in all notes that takes longer than two seconds is listed in the background-work indicator as "Searching…", with Cancel: it stops the search and keeps the last list (#622).
- The LaTeX export now writes a sound or a video on a line of its own as its player card (the cover or the first frame, the play button, the name and the times, with the caption below) instead of a red "not in a PDF" line, drawn like the built-in export does; quotes get their violet bar, tags are violet on their light field, and emoji such as the 📅 of a due date are set in an emoji font that travels with the document instead of showing a missing-glyph box (#623).
- The suggestions of the filter form (tags, property names and values) are collected on a background thread instead of the window's: a very large vault no longer pauses when you first click into a field. The list shows "Loading…" until the first set is ready, and the previous suggestions stay while a changed vault is read again (#636).
- The peek of a board card closes with a click anywhere outside it (what you typed is saved); a click on another card opens that card's peek in one click. Dragging a card, the peek's own menus and the lane settings do not close it (#639).
- Publishing a vault is now locked: while one publish runs (from the Publish button, from `shale export-site --publish` or in another Shale), a second publish of the same vault stops at once with "Another publish of this vault is running" and changes nothing. A lock left by a crashed Shale never blocks (#661).

### Fixed

- The graph view's physics no longer allocates a new force list and a new quadtree on every step, which cuts the allocations of the layout and of a graph frame (#402).
- A quoted property value shows without its YAML escapes: the sender of an imported mail is `"Nils Mundhenke" <nils@example.org>` in the properties summary and card, not `\"Nils Mundhenke\"`; a comma inside quotes in a `[a, "b, c"]` list no longer splits the item (#557).
- In the right sidebar, the mouse wheel and the touchpad scroll each of the stacked views (and the panel of a ribbon shortcut) again; before, only the scrollbar worked (#558).
- Renaming or moving a note, file or folder keeps every link form working, now tested for wikilinks with alias or heading, embeds of pictures, PDFs and attachments, Markdown links and images with relative or URL-encoded paths, and links in properties; a note with unsaved edits keeps them while its links are updated, and a rename with all its rewritten notes is one history snapshot (#567).
- The calendar in the right sidebar no longer draws its buttons over the month name when the sidebar is narrow: the buttons wrap to a line below the name, and the Month/Week/Day and S/M/L buttons wrap too (#569).
- While Settings or another dialog is open, Shale's shortcuts and the editor's keys no longer reach the app behind it; Esc and the dialog's own keys still work (#572).
- A sound you attach to a mail goes out as `audio/wav`, `audio/ogg`, `audio/flac`, … and not as a plain file; before, only mp3 did (#579).
- A WAV file that VLC plays no longer shows "The file is damaged or not a sound file": the A-law and µ-law WAV files of phone systems and answering machines (FRITZ!Box call recordings, 16-byte format chunk) now play. Sounds Shale cannot decode itself (opus, ADPCM) play through ffmpeg when it is installed, and the player names what a file holds instead of calling it damaged (#581).
- Feeds: a like or boost is also written into the post search index, so a post opened from the search no longer shows an old liked / boosted state or old counts; a post opened from the search prefers the copy a feed holds (#588).
- A Markdown link or image with a title, `[x](Note.md "title")`, `[x](Note.md 'title')`, `[x](Note.md (title))` or `![x](pic.png "title")`, is now read as a link: it resolves, shows as a link in the editor and the reading view, counts in backlinks and the graph, and follows a rename or move of its target, which changes only the address and keeps the title (#591).
- When a rename could not update a note because that note's unsaved changes could not be saved first, the message now says so instead of "changed by another program meanwhile" (#591).
- Import: the mails or posts of one thread that are dropped at once now link to each other with `previous` and `next`, in date order together with the notes already in the vault, like importing them one by one. An item that continues another item of the same import shows "Continues: <title> (in this import)"; the question for older mails is asked once per thread (#594).
- In the settings of a board lane, "+ Add filter" and "+ Add sort" closed the popover without adding the row. The settings now stay open until closed with the ✕, Esc or the ⋯ button; a click outside no longer closes them (#632).
- A card whose file name has a `#` (an imported mail note "… #2583-3353-7982.md") stays in the lane you drop it on: `[[…#…]]` is a heading link, so the pin did not find the note and the card vanished from the board. Names that `[[ ]]` cannot hold are pinned as a markdown link with the note's path, and a pin that finds no note is named under the lane's heading instead of being skipped silently (#637).
- The LaTeX export of a board no longer fails with "package geometry is missing" on a TeX Live that Shale installed before the board export existed. Settings → LaTeX and the export dialog now say "The managed TeX Live misses: geometry" with an **Install** button (`tlmgr install` of exactly the missing packages, with progress and Cancel), a board waits for it before it can be exported, **Update** installs missing packages too, and a compile that fails for such a package points to the button (#640).
- A board exported through a LaTeX template now has real landscape pages: the pages of the board are landscape (not a portrait page with turned content), so the template's header (the tuda-ci bar) and the page number stay at the top and bottom of the page as on the other pages, and the notes after the board continue on portrait pages with the normal layout (#641). The `pdflscape` package is no longer used.
- The board's columns are now one table that LaTeX breaks at the real page end, so a landscape page holds as many cards as fit instead of a third of them, and the table's heads repeat with "(continued)" (#641).
- A publish could be refused as "Another publish of this vault is running" while Shale was starting another program; a taken publish lock is now asked for again for up to half a second before it counts as held (#659).

### Security

- Updates are only installed when the download is signed. The owner signs every release file with an ed25519 key whose public half is built into Shale (an SSH signature, `<file>.sig`, checked with `ring`); the SHA-256 check stays. A release that has no signature yet (older releases, or the hours before it is signed) is only announced, with a link to its page; a download with a wrong signature is deleted, nothing is installed and a warning says why. `scripts/sign-release.sh` signs a published release (#422).

## [0.7.6] - 2026-11-23

### Fixed

- For developers: the test that only one vault scan runs at a time waits for the first scan to start before it counts, so it no longer fails on a slower computer (#554).

## [0.7.5] - 2026-11-23

### Fixed

- For developers: the tests that bind a Unix socket do so through a short path, so they pass where the temp folder has a long name (macOS) (#551); the thinning test uses a fixed date, so it passes at any hour (#552), and a paste test builds its expected Windows link the way Shale writes it (#545).

## [0.7.4] - 2026-11-22

### Added

- The Import dialog leaves out the noise of an e-mail's signature: the signature files of a signed mail (`smime.p7s`, `signature.asc`) and the pictures of signature blocks (logos, social icons, `image001.png`, pictures that are only used after the `-- ` line) start unticked and say why, and you can still tick them. The box **Skip signatures** (also in Settings → Import, on by default) turns this off; a real picture or file attached to the mail stays ticked (IMP-059, IMP-060, IMP-061, CFG-074, #440).
- Conflict copies of Syncthing, Dropbox, Nextcloud, ownCloud and Seafile (`Note.sync-conflict-….md`, `Note (… conflicted copy …).md`, `Note_conflict-….md`, `Note (SFConflict …).md`) and Shale's own `Note (conflict …).md` are recognised. They carry a "conflict" badge in the file list, and the note shows a banner "Another device changed this note at the same time" with Compare (the changes, or the Markdown side by side), "Keep this one" (its text goes into the original) and "Keep the other"; the copy goes to the trash, and the text it replaces goes there too, never deleted for good. The original note shows the same banner and a badge. While the original exists, the copy is left out of search, links, the graph, backlinks, tasks and tags. Copies of OneDrive, iCloud and Google Drive (`Note 2.md`) cannot be told from a real note by name and stay ordinary notes (NET-015, #475).
- For developers: the SMB storage checks run without root against the Samba test server through an rclone FUSE mount (not the kernel CIFS client); the kernel SMB and NFS mounts stay a manual run (#477).
- For developers: property tests (`proptest`) for the history diff (HIST-009…HIST-016): the edit script turns `a` into `b` and keeps as many elements as the longest common subsequence, the comparison, the side-by-side source and the word spans hold every line or word of both texts once and in order, folding hides only unchanged rows, change starts are the first rows of the runs, and the patch applied to the old text gives the new text (#532, #534).

### Changed

- `.avi` and `.ogv` videos now have the video player like the other video types (needs ffmpeg, as before), and one list of media extensions serves the import, the player and the Git LFS patterns: Shale's `.gitattributes` block now also sends avi and ogv files to LFS (MEDIA-001, MEDIA-009, SYNC-017, #206).
- File history: a history kept on this computer (no git server) is thinned once a day: every version of the last day, one per hour for a month, one per day before that; restored and deleted notes stay. A vault's own git repository is never rewritten (#479).
- Connecting a server to a vault whose history was kept on this computer now moves those versions into the vault's repository, so they go to the server with the first sync and stay in the history panel (#485).
- For developers: `scripts/land.sh` refuses a change to a property test of a spec rule unless the rule's row in `docs/spec/` changes too or a commit says `rule-unchanged: <ID> <why>` (DEV-043, #533).
- For developers: the spec says what gates a release on macOS and Windows: GitHub's main-check skips a release commit, so the release build's own macOS and Windows tests are the gate (#546).
- For developers: the `lead` skill starts implementing subagents in Claude with `isolation: "worktree"` instead of a hand-made worktree, which the desktop app refused for new files; the hand-made worktree stays for opencode (FORGE-022, #550).

### Fixed

- In a vault that Shale finds in a cloud-synced or network folder (Syncthing, Dropbox, Nextcloud, OneDrive, a share), the open tabs (`.shale/state.json`) and the undo histories (`.shale/undo/`) are kept outside the vault, in Shale's data folder under the vault's ID, so two computers no longer overwrite each other's tabs or leave `state.sync-conflict-….json` files behind. What an older version left in the vault is moved out on the first open. Shale also removes `.shale-tmp-*` files that a crash left (older than a day) and conflict copies of the state and undo files; notes and the settings are never touched (NET-016, #476).
- The vault rescan uses about 13 % less memory at its peak again (it copied every path for its worker since NET-013) (#521).
- The Word import and export stop pandoc when it needs more than 2 GB for a file and say so, instead of letting a broken document use up the computer's memory; the developer corpus run gives up on such a file too (#528).
- A folder path typed as `/~` (or `..~`) made a folder named `~`, which Shale then refused to read back as typed; it is now refused like `~/x` (#537).
- For developers: property tests (`proptest`) for rules that hold for all inputs, with a shared fixed-seed setup (DEV-042); the first ones check that typed vault paths never leave the vault (#532, #537).
- Emptying the first cell of a table row written without outer pipes (`a|b`) made the cell vanish, so the others moved left; emptying the last cell of such a row did the same. The cell is now written with its pipe (#535).
- A table column grid given no room and columns with no minimum width got a width of "not a number" instead of nothing; it is now zero (#535).
- For developers: property tests for the table text: a cell set reads back and the others stay, the comment line round-trips, cell ranges lie in the line, column widths keep their bounds, and the table finders never leave the note (#535, DEV-042).
- A calendar file with a time like `20261112T1é000Z` (a non-ASCII character among the digits of a time) or with a duration of an absurd size (`P99999999999999999W`) made Shale stop; such a time or duration is now ignored (#536).
- For developers: property tests for the iCalendar reader and writer: what is written is read back, every line is folded to 75 octets, the repeat rules of the event dialog are read back, occurrences lie in their window and no text makes the reader panic (#536).
- A note with a `$` just before a letter such as `é` or `ü` (as in `$é`) made Shale stop with an error when it looked for math in it; it does not any more (#538).
- A markdown link whose file name has a `#`, written `a%23b.md`, was read as the name `a` with the heading `b.md`, and renaming such a note wrote links that did not read back; the `%23` now stays part of the name (#538).
- For developers: property tests (`proptest`) check that the Markdown scanners (links, tasks, tags, footnotes, math, headings, wrappers) never panic and return ranges inside the text, and that replacing a link's target keeps the rest of the text (LINK-018, #538).
- Redoing the deletion of the last lines of a note could hand vim-lite a cursor position past the end of the text (it was clamped there, so nothing showed); undo and redo now return a position inside the text (#539).
- For developers: property tests for undo and redo (every checkpoint comes back in reverse, redo gives back the text before the undo, a new change drops the redo steps, the 200-step cap, the history read back from the vault) (#539).
- In a table, a lone backtick typed into a cell no longer pairs with a backtick in the next cell and merges the two cells; it is written as `\`` (#540).
- An unclosed `[x](` earlier on a line could take in a later link once that link's target was replaced (a rename); a markdown link's destination now follows CommonMark (balanced parentheses, `<…>` without `<`), and an unclosed opener is plain text (#541).
- For developers: the tests that compare a folder recorded by a fake program with one Shale built resolve links first, so they pass on macOS, where `/var` is a link to `/private/var` (#542).
- Windows: links to files outside the vault are written as `file:///C:/…` and paths are shown without the `\\?\` prefix and with one kind of separator; a written link is found again (#544).
- For developers: the Windows tests for outside-media links, paths shown in messages, the Word schema cache and a git folder remote with spaces now expect Windows paths, so they pass on Windows (#545).
- For developers: the history diff's property tests now also run with a tiny cell limit, so the fallback for a middle part that is too big to compare (`MAX_CELLS`) is checked for many inputs (#547).
- For developers: the test of the Word export job (DOCX-002) no longer fails now and then with "Text file busy": a cancelled fake pandoc can still run for a moment, so each fake pandoc in the test has a folder of its own instead of the file being written again (#549).

## [0.7.3] - 2026-11-22

### Changed

- The first code block of a language no longer stalls the window: the grammars of the common languages (TypeScript, JavaScript, Rust, Python, shell, JSON, TOML, YAML, C++, Go, HTML, CSS, Markdown) are compiled on a background thread right after start. The first TypeScript block took about 300 ms on the UI thread before (#245).
- The check of the mail index now starts with the window when a mail account is set up, as a background task "Checking the mail index…" in the busy indicator, so the mail view finds the index ready; opened earlier, the view shows that text with a spinner instead of "Opening the local mail copy…". The integrity check itself stays (#385).

### Fixed

- For developers: the media UI tests wait for the picture workers (MEDIA-024) to finish instead of a fixed number of frames, so they pass on slow and instrumented builds (#529).
- For developers: the file look-up test waits for the window's repaint after a worker answer instead of checking it at once, so it no longer fails now and then under load (#529).

## [0.7.2] - 2026-11-22

### Added

- Work that runs longer than a moment now shows what it does and cannot be started twice. A small indicator in the top right of the editor pane (in the row of the tabs) lists what runs in the background (a spinner and the text, or "2 tasks running"; point at it or click it for the list with how long each runs and **Cancel** where the work can stop). The git sync ("Syncing with git…"), the Git LFS setup, the check of file names in Settings → Vault (new: **Cancel** stops it) and the PDF export of a folder are the first on it; the buttons that would start them again are off while they run. The other long actions follow (#397).

### Changed

- The headless window tests share one wait for the background jobs after `ShaleApp::build` (DEV-041): `Headless::settle` frames until the tag learner, the rescan, the vault check and the other jobs are done, with a deadline and a message that names what still runs. The tests of the settings file, of dragging in the file list, of the sync wizard and of the file history use it, so a file such as `.shale/tag-words.json` no longer appears in the middle of a test on a loaded machine (#272). `media_022` waits for the new vault check job with the same wait instead of reading the answer of the first check as the answer of the second (#514).
- Test scripts that stand in for external programs (pretend gpg, pandoc, ffprobe, `secret-tool`, git hooks) are written by one helper (`test_exec::write_script`, DEV-040) that starts each script once before the test does, until it is no longer "Text file busy", so a test that forks at the same moment no longer fails the next test with `ExecutableFileBusy`; the wait of `media_022` for the vault check ends by the clock instead of after 300 frames (#514).

### Fixed

- On Windows, no console window flashes up any more when Shale starts `git`, `git-lfs`, `ffmpeg`, `ffprobe`, `gpg`, `curl`, `pandoc` or the TeX tools: every program is started with the creation flag `CREATE_NO_WINDOW`, set in one place (`program::command`), and a test checks that no other code starts a program (#205). Not yet seen on a Windows desktop.
- For developers: the release-script tests in Forgejo's build job get `jq` before they run, and stop with a clear message when it is missing (#522); `--allow-perf-regression` now also lets a release pass when the re-run confirms a regression (#520).

## [0.7.1] - 2026-11-22

### Added

- For developers: after a release is published, GitHub runs the `full` benchmark profile on Linux, macOS and Windows, keeps each result as a release asset and writes what is odd compared with the previous release of the same system into the job summary. It is statistics only and never fails or delays a release (#408).
- Settings has a search over all settings, at the top: it finds a setting by its name, its description or its tab, and a key action by its name or its keys (`ctrl+h`, `F6`). ↓ / ↑ and Enter, or a click, show the tab, scroll to the setting and pulse a box around it (steady where animations are off). Ctrl+F or `/` focuses the field. Every setting of the dialog is in one table that tests keep complete (#444).
- The Keybindings tab has a filter field (Ctrl+F or `/`): it keeps the actions whose name or keys fit, hides empty groups and says "No action matches"; Reset all still resets every action (#444).
- A note whose headings skip levels (`##` first, or a `#` followed by a `###`) now shows a thin bar above its text, in the editor and in the reading view, that names what is missing ("Headings: no level 1 heading", "level 2 is skipped"). When the highest heading is deeper than `#`, a button **Move all up by X** moves every heading up by the same X in one undo step. The **×** hides the bar for that note until Shale quits (#457).
- A note can be exported as a Word file (`.docx`): **⋯ → Export to Word…** in the note's menu, or in the context menu of a note in the file list. It is the reverse of the Word import and uses the same pandoc (the Word import setting does not have to be on). The dialog saves to `<vault>/exports/<name>.docx`, has the boxes "Use the original Word file as the template" (for a note imported from Word with its original kept: its styles, fonts, numbering, headers and footers come back), "Table of contents" and "Open the file afterwards", and runs in the background. Highlights, text and background colours, underline, sub- and superscript, strike-through, pictures at the size you see with their captions, page breaks, centred, right and justified paragraphs, the import's title block, lists, tables, footnotes and math come through; a note's `print-*` properties set the page, font, spacing and headings of the file. What the Word import lost (merged cells, figure numbers, tab stops, small caps) cannot come back (#487).
- For developers: `scripts/roundtrip-docx.sh <file.docx>` imports a Word file, exports the note again and prints the share of the words that came back (#487).
- For developers: `scripts/validate-docx.sh <file.docx>…` checks the parts of a Word file against Microsoft's WordprocessingML schema; the Word export tests check every file they write with it (the schemas are fetched into a cache outside the repository, `scripts/land.sh` fetches them before the tests, and the check is skipped without them). `scripts/docx-corpus.sh` runs the round trip, the schema check and a page count on 50 real-world Word files of pandoc, LibreOffice, Apache POI and the Open XML SDK and prints a table (#497).
- The fixture `styled.docx` of the import tests is now valid WordprocessingML (its Heading 1 had `w:pageBreakBefore` after `w:spacing`) (#497).
- A note can show pictures, PDFs, sounds and videos that stay outside the vault, without copying them into `z_media/` (#499, first of three parts). Two link forms: a **library link** `![[@Videos/2024/a.mp4]]`, where `Videos` is a folder each computer maps in its own settings (`media_libraries`), so the same note works on every device; and a **file link** `![](<file:///home/ada/Videos/a.mp4>)`, Obsidian's form (also `%20` and `file:///C:/…`). They show in the editor, the preview, the reading view, the link preview and the PDF export, with players for sound and video. A missing file or a library that is not set up on this computer says so in place of the embed; a click on such a link opens the file in your system's program and never makes a note. The setting that chooses between copying and linking, and the Settings page for library folders, follow in #500 and #501 (see ADR-0030).
- **Files from outside the vault: copy or link** (#500, second of three parts, ADR-0030). A new setting in **Settings → Import**, "Files from outside the vault", says what a pick with `[!`, a paste or an import does with a picture, PDF, sound or video that is not in the vault: **Ask** (the start value), **Copy into the vault** (as before) or **Link where they are**. The same page has the **library folders** of this computer: a name and a folder per line, with Add library and Remove, and the name checked as you type. A linked file under a library folder is written `![[@Videos/2024/a.mp4]]`, any other file `![](<file:///…>)` (a PDF as a plain link); both forms are read back by Shale and Obsidian. Nothing is linked because it lies in a library folder: every entry point follows the setting. The Import dialog has a **Copy into the vault / Link where it is** choice for each such file and one for all of them; the `[!` picker shows what a pick does in its last line, **Alt+Shift+L** changes it for this pick, and with Ask it asks after the pick (C, L, Esc or the buttons); pasting files with Ask opens a small question first. Other files are always copied. The user guide and the settings reference describe it.
- **Media outside the vault: copy into the vault, checks, data usage, sync note** (#501, last of three parts, ADR-0030). A picture, PDF, sound or video that a note links from outside the vault can be **copied into the vault** later: the caption of the embed in the editor and the preview has **Copy into the vault** at its right end, the reading view has a button beside **Open** (a line under a player), and the ⋯ menu of a note has **Copy outside media into the vault** for all of its links whose file is there. The files go to `z_media/` in the background and the links are rewritten (width, alias and form stay; in the editor one step for Ctrl+Z); files that are not there are named in the message and left. **Settings → Vault → Check names for Windows and macOS…** also lists the vault's links to outside media by note (a file not found, a library not set up on this computer or whose folder is not there, file links that work on this computer only), and **Data usage** shows how many outside files the vault links, each once, with their size and how many are missing; both look the files up in background jobs. Outside files are not synced or kept in the file history (checked by tests); **Settings → Sync** says how many media files the vault links from outside, and the caption of such a file, and the name in the reading view, say "Outside the vault: not synced" on hover. The user guide describes it.

### Changed

- The Forgejo CI jobs pass the job token of their checkouts, and the mirror job the GitHub token, through git's environment instead of `git -c http.extraHeader=…`, so no token shows in git's arguments in the process list (#149).
- A picture embedded inside a line of text (`an icon ![[icon.png]] here`) is now drawn in the line in the reading view and in the built-in PDF export, as the LaTeX export already did: 0.9 of the line high, set a little below the baseline like a letter, never wider than the line. Before, both showed its link text. A picture that is missing or cannot be read, and any other embed inside a line, still show their link text (#314).
- The CI job of the real-server tests retries the install of `podman-compose` three times and fails its step with the reason when it still cannot be installed, instead of failing later with "neither podman-compose nor docker compose" (#342).
- For developers: the flamegraphs of a release are no longer a folder in git but an attachment of the release, `shale-flamegraphs-<version>.tar.gz`, made by the Linux benchmark job of the release build for the scenarios that were flagged or are the slowest. The performance page `docs/perf/index.html` links to the attachment of its latest release instead of to `docs/perf/flamegraphs/` (#400).
- For developers: the performance check of a release (`scripts/bench-release.sh`) no longer stops the release for noise. A time regression that the stored comparison flags is measured again alone against the previous release, with 5 interleaved runs of each side, and stops the release only if it shows again; allocation and memory regressions, which are exact, stop it as before. The numbers of the re-run are kept in `docs/perf/compare/` and commented on the release issue, whatever they say (#408, #493).
- A check of the specs (DEV-039): every test that a spec row names must carry that row's ID in its name or in a `// spec: ID` comment above it, so a row cannot name a test that is about something else. The tests of today that do not yet are listed in `tests/spec_refs_allowlist.txt`, which can only shrink; the rows of `engineering.md` have none left. New tests check what BUILD-019 says (the real program ends a wrong command line with the message, the usage and exit code 2; `regen.sh` refuses an older binary) and the layout of DEV-008; DEV-008 no longer states a field count of `ShaleApp` that was out of date (#438).
- The search of the mail view (`/`) now reads your text like the mail part of Space f g and the sidebar search: every word matches the start of a word (`meet` finds "meeting"), and `*` and `?` work as wildcards; "phrases" stay exact. Its hint now also names `cc:` and `body:`, which already worked (#490).
- Developers only: `CLAUDE.md` now states how agent sessions of opencode and Claude desktop work side by side (the main checkout stays on `main`, one issue per branch and worktree, a claim comment on the issue, when a worktree may be removed), and a project skill `lead` (`.claude/skills/lead/SKILL.md`, read by both tools) orchestrates a batch of issues from one session (#496).

### Fixed

- A click on the progress bar of a video before its length is known (the total still shows `–:––`) is no longer ignored: the bar shows where you clicked and the video jumps there as soon as the length arrives (#258).
- On Windows, the `README.txt` that **Save the .tex** writes for a failed LaTeX export gave a Unix command (`TEXINPUTS="…" lualatex main.tex`), which neither cmd nor PowerShell runs. It now gives the commands of the system: `set "TEXINPUTS=…//;"` for cmd and `$env:TEXINPUTS="…//;"` for PowerShell on Windows (TeX separates search paths with `;` there), each followed by `lualatex main.tex` (#312).
- In the built-in PDF export a formula that fell at the start of a wrapped line was left out; it is now drawn (#314).
- In the LaTeX export of a folder, a top note whose `latex-template:` names a kind a book cannot use (`tuda/beamer`: slides, letters and posters take a single note) now gets the red line of a missing template, which says what is used instead; before, the dialog fell back to another kind without a word (#319).
- Windows: starting `shale.exe` from Explorer or a shortcut no longer opens a console window next to the app. Started from a terminal it still prints its usage and errors there; `cmd.exe` does not wait for it, so use `start /wait shale.exe …` when a script needs the exit code of `--export-pdf` (#328).
- A new note name that starts with a mix of dots and spaces (`. ..Name`) no longer keeps a leading dot, which made the note a hidden file that Shale then ignored; the leading dots and spaces are all removed, as the rule says (#437).
- A `#` line inside a `$$` math block is no longer taken for a heading by the outline, the heading links and the lift of headings (#457).
- The 3-second look for changes in the vault folder no longer freezes the window on network shares and cloud folders: the walk, the file checks and the reads happen in the background, and only the result is applied in the window. A scan that took long makes the next one wait ten times as long (never less than 3 seconds), and a result that is out of date (you saved the note meanwhile, or the vault changed) is dropped instead of overwriting newer text (#473).
- Links now find a note whether its name and the link are written with the same Unicode normal form or not: `[[Café]]` typed on Linux finds `Café.md` made on a Mac (accent as `e` + combining mark). Backlinks, the graph, `[[` completion, the quick switcher and rename with link update treat both spellings as one name; the file name on disk is never changed (#474).
- Sync: a folder as the remote whose path has a space (`…/Dropbox/My notes.git`, `C:\Users\Ada\OneDrive - Company\notes.git`) is accepted again instead of "A URL can't contain spaces or control characters", in Settings → Sync and in the Turn on sync wizard. An `https://`, `ssh://` or `git@host:path` address with a space, a control character, or a leading `-` is still refused (#494).
- A Word file (or a CalDAV answer) whose XML is nested thousands of levels deep no longer crashes Shale with a stack overflow: the Word import refuses it at once with "the XML is nested too deeply (more than 256 levels)", the Word export gives an error, and the calendar reports that the answer cannot be read (#503).
- The macOS download (dmg) is made with an explicit volume size, so the release build no longer fails with "No space left on device" when the app grows (#502).
- **Pictures and outside media no longer freeze the window on a slow drive** (#504). Whether a shown file is there, when it was changed and how a picture decodes were asked of the disk on the window's thread in every frame, so a NAS, a share or a USB disk that did not answer (a sleeping disk, an unreachable server) stopped the whole window. Now the disk is asked on a background thread, the answer is kept and asked again every few seconds without anyone waiting, pictures are decoded in the background (a few at a time), and an embed shows a quiet "Loading …" strip until its file is looked at (the editor, the reading view and the link preview). A file that appears, goes or changes is noticed within a few seconds. A click on an outside link, and a video that opens in the system's player, also hand the file to the program in the background.

### Security

- Windows: Shale's private files and folders (the settings file, the lock file, debug logs, the private temp folders and the folders of the mail store) now get an access list of their own that only the user and `SYSTEM` are on, instead of relying on the one they inherit from the user profile; a data folder outside the profile is covered too. A folder that already has files in it keeps its list (#74).
- The `url` library is updated to 2.5.9, which fixes a stack overflow on nested `blob:` addresses (RUSTSEC-2026-0592) (#495).

## [0.7.0] - 2026-11-22

### Added

- Git for sync can be installed with one button. Settings → Sync shows which git and git-lfs it found and which is used, offers **Download and install git** with the download size, the size on disk and the folder `<data folder>/shale/git` (nothing is downloaded before you press it), checks every file against a pinned SHA-256, builds the folder aside and renames it when it runs, and has Cancel, **Remove…**, a choice between the git of your computer and Shale's own (`git.choice`) and an opt-in check for newer versions that can also install them (`git.updates`, off by default). Windows gets MinGit and git-lfs, macOS opens Apple's installer for git and downloads git-lfs, Linux shows your distribution's install command and downloads git-lfs (#468).
- **Turn on sync**: Settings → Sync has a button **Turn on sync…** that asks how you want to keep your notes safe: *History on this device only*, *My folder is already synced by another app* (Nextcloud, Dropbox, OneDrive, iCloud, a network drive: the history stays on each computer and no `.git` folder goes into the folder), or *Sync with a server* (GitHub, GitLab, Codeberg, Forgejo, Gitea or any git address). Without git the first step offers the install. For a server you sign in with a personal access token (kept in the system keyring, never in a file, an address or a command line; git gets it through Shale's own askpass helper) or an SSH key (Shale can make one and shows the public key to copy), then create a private repository or pick one (Forgejo, Gitea and GitHub), read in plain words what the first sync will do, and run a test sync that reports "worked", a wrong token, no network, no repository and the like (#469). The address field and **Connect** are kept under **Advanced**.
- Shale now notices when a vault is in a network folder (NFS, SMB, WebDAV, a Windows network drive) or in a folder that Dropbox, Nextcloud, ownCloud, Syncthing, Google Drive, iCloud or OneDrive syncs, and says so in Settings → Sync and in the wizard (a warning only; the history of a vault without `.git` already stays outside the vault). A vault with a `.git` folder inside such a folder is warned about the risk, and when it has no server it is offered **Move the repository out**, which copies the repository to a private place on this computer, checks the copy, and only then removes `.git` from the vault (#469).
- A user guide note, "Vaults in cloud and network folders", says what is safe (saves, conflict copies, changes from other computers) and what to avoid (a git repository in a synced folder, a big vault on a slow share) for vaults on network drives and in Nextcloud, Dropbox, OneDrive, iCloud and Syncthing folders. The findings behind it are in `docs/storage-on-shares.md` (#470).
- Developers only: test stacks `tests/syncthing-env/` (two Syncthing devices, rootless), `tests/smb-env/` (Samba, mount needs root) and `tests/nfs-env/` (NFS, needs root, not run yet), with ignored tests that run Shale's save, scan and conflict code on them (`netshare_env_tests.rs`), and the spec section NET (#470).
- File history: Shale keeps earlier versions of your notes and drawings, with or without a git server. A version is saved when you save a note (at most one per note every 10 minutes while you type), when you stop typing for a minute, when you close the note and when the window closes. A vault that syncs with git keeps them in its own repository (the sync pushes them as they are); any other vault keeps them in `<data folder>/shale/history/<vault id>`, outside the vault, tied to it by a small file `.shale/history-id`. A note's ⋯ menu has **Versions of this note…** with **Restore** (as one new change you can undo; the text before it is kept first), **Copy text** and **Read-only**, and Settings → Sync has **Restore a deleted note…**. Settings → Sync switches the history off and sets the two times (`history.enabled`, `history.coalesce_minutes`, `history.idle_seconds`); Data usage shows the size of a vault's history (#471).
- Sync commits and version commits now end with the lines `Shale-Reason:` and `Shale-Device:` (#471).
- A file history panel and a comparison view that is not a git tool. The history of the open note is a slim column in the empty right margin of the note when the window is wide enough (one note, **Readable line length** on), else a **History** panel of the right sidebar (a clock icon, next to the outline and the calendar); the **×** of the column hides it and `history.show_panel` remembers that. The list is grouped by day (Today, Yesterday, dates) with relative times, where each version came from in plain words (edited here, synced from your laptop, restored version of 17 Nov 09:00, merged after a conflict, renamed from …), and a small bar of +/− ticks whose hover says "12 lines added, 3 removed". A click opens **Changes since 11:05**: the note with inserted words highlighted and underlined, deleted words struck through, a mark (+, −, ~) in the gutter of every changed block and the unchanged parts folded ("… 34 unchanged paragraphs"), a time slider and a "to" list to compare any two versions, **Next change** and **Previous change**, **Restore this version**, **Copy text** and **Read-only**. A switch shows the Markdown side by side with line numbers and word highlights, and **Copy as patch**. Drawings show before and after thumbnails. The ⋯ menu entry is now **History of this note** (#472).
- The search over notes, mail and posts (Ctrl+Shift+F and `Space f g`) can look in fewer sources. A prefix at the start of the query chooses them: `f:` notes, `m:` mail, `s:` posts, also combined (`fm:`), with or without a space after the colon. A button per source in the header shows its count ("Notes 12 · Mail 20+ · Posts 4", also for a source that is switched off, in grey): a click lists only that source (again: back), Shift+click switches it in or out of the default selection, and Tab / Shift+Tab step through the sources. The default selection is saved and is the same in the sidebar and the picker. Settings has a new Search section for the three letters (refused when they clash with each other or would spell a field such as `to:` or `in:`) and the default selection (#478).
- The user guide has a note "Search syntax" with everything you can type in a search: what notes, mail and posts each read (the whole text with `*` and `?` for notes; all words, `"phrases"`, fields and prefix wildcards for mail and posts), the source prefixes and buttons, the places to search and how the hits look (#484).

### Changed

- The key action "Search all notes" (Ctrl+Shift+F) is now called "Search notes, mail and posts" (#478).
- The key hint of the pickers (`Space f f`, `Space f g`) and of the Ctrl+O quick switcher is now at the top, between the search field and the list, and stays in view whatever the length of the list: the field and the hint stay put and only the list scrolls. The hint shows the keys as they are bound now, and the keys of these windows (next line ↓ / Ctrl+J, previous line ↑ / Ctrl+K, open Enter, close Esc) can be changed in Settings → Keybindings under the new scope "List windows" (also for the font list) (#481).
- The sidebar search (Ctrl+Shift+F) is now the same search as `Space f g`: it also lists mail (while the mail store is open) and the posts of your feeds, grouped under Notes, Mail and Posts with their counts, takes the wildcards `*` and `?` and the field prefixes of mail and posts, and waits for your typing to settle like the picker does. The keyboard reaches every group: j / k go from hit to hit, Ctrl+J / Ctrl+K to the first hit of the next / previous note or, in mail and posts, group, and Enter opens the hit where it belongs (the note at the found text, the mail in the mail view, the post in its feed). The search field now says "Search notes, mail and posts" (#482).
- The lines of notes that the search finds (Ctrl+Shift+F and `Space f g`) are now shown rendered like the editor draws them: bold, italic, links and wiki links by their shown text, tags, task boxes (`[ ]` / `[x]`), bullets and headings without their markup, with the found text marked (also for wildcards). Mail and post hits mark the words of your query. New setting **Settings → Search → Hits of notes** (`search.hit_display`): follow the editor (default: rendered while the Markdown preview is on or the open note is in the reading view, raw otherwise), raw, or rendered (#483).
- The user guide shows the search windows (`Space f g`, the sidebar search) and Settings → Search in pictures, and the pictures of the Settings tabs and of the keybindings show the new Search tab and key names (#481, #478, #483).
- In the search over everything (Ctrl+Shift+F and `Space f g`) every word of a mail or post query now matches the start of a word: `meet` finds "meeting", `from:ann` finds the sender "Anna". A "quoted phrase" stays exact, and the found words are marked in the hit. The mail view's own search is unchanged (#491).
- A task in the editor's Markdown preview and in a rendered search hit is now drawn with a real checkbox (ticked for a done task) instead of `- [ ]` / `[x]` as text. Clicking the box in the preview still ticks the task (#489).
- A list item in the editor's Markdown preview and in a rendered search hit is now drawn with a painted dot (filled on the first level, hollow on the next, alternating) instead of its `-`, `*` or `+`. Numbered items keep their numbers, tasks keep their box, and the line with the cursor stays raw (#492).

### Fixed

- For developers: the test suite now passes when run as root (as in the CI containers) and under parallel load in release builds. Flaky UI waits in the sync, managed-git and file-history tests were replaced by waiting for the background jobs to finish, two history tests no longer share a vault folder, and the move-out failure test no longer relies on an unreadable file (#469, #468, #471).
- For developers: the storage check NET-001 now tests the file lock from a second process with Shale's own test binary instead of `flock(1)`, which macOS does not have, so the test suite passes on macOS again (#470).
- For developers: the askpass helper's integration test, which fakes the Linux keyring (`secret-tool`), now runs only on Linux; on macOS and Windows the helper is covered by its unit tests, so the test suite passes on macOS again (#469).
- For developers: the mail test that checks a Bcc copy no longer fails now and then. It looked for "bcc" anywhere in Shale's own message headers, so it matched random hex in a message ID or boundary. It now looks for a `Bcc:` header line (#488).

## [0.6.10] - 2026-11-21

### Added

- GitHub issue forms for a bug report and a feature request, a `CONTRIBUTING.md` (how to report, how changes are made, how a pull request reaches `main`, labels, licence) and a `CODE_OF_CONDUCT.md` (#456).

### Changed

- Developers only: the podman test stacks (mail, CalDAV, TeX image) are used by one checkout at a time. Everything that starts or changes them, `scripts/land.sh`, the helper scripts and the ignored `cargo test` suites, takes one lock per user and machine and a second checkout waits for the first, so a suite no longer fails because another worktree restarted the stacks or added mail (#420, #417).
- Developers only: the icon-button hints of a `--keys` script run (BUILD-007) belong to the window that runs the script instead of one process-wide list, so a test that draws icon buttons can no longer feed or empty the hints of another test; `build_007_an_icon_button_is_found_by_its_tooltip_and_only_while_a_script_runs` no longer fails now and then in a parallel `cargo test` (#429).
- The notes of the user guide have proper headings: the headings of each note start at level 1 (`#`) instead of level 2, so the outline of a note and its table of contents begin at the top (#446).
- Landing a change (`scripts/land.sh`) runs only the real-server suites the change touches: CalDAV code starts only the CalDAV stack, mail code only the mail stack, and the TeX image is built only for TeX changes. Before, any one of them started both stacks, built the TeX image and ran all three suites. The tag CI still runs all of them (#447).
- The real-server suites of a landing (`env-tests.sh run`) run at the same time: the CalDAV and mail stacks and the TeX image build start together, then every suite runs with its own log (printed whole when the suite ends), so a landing that needs two or three suites takes about as long as the slowest one instead of the sum. A failing suite is named and the others still finish (#448).
- Landing a change (`scripts/land.sh`) runs the benchmark tests (`cargo test --features bench bench::`, about a minute) only when the change touches the benchmark harness or the build (`src/bench/`, `src/main.rs`, `scripts/bench*.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `scripts/land.sh`), and always for a release landing. The clippy check of the benchmark feature still runs on every landing, so a change anywhere that breaks the harness' build is still caught (#449).
- Developers only: `scripts/land.sh` runs the tests of a landing with `cargo nextest run --locked --no-fail-fast` instead of `cargo test`, about 18 s faster (38 s instead of 57 s at 14 test threads); it passes the thread budget (`RUST_TEST_THREADS`, or the one in the cargo config's `[env]`) on as `--test-threads`. It needs cargo-nextest (`cargo install cargo-nextest --locked`) and stops at the start without it. The step is named `cargo nextest run` in the landing statistics (#451).
- Builds: GitHub now tests macOS and Windows for each check tag. A new workflow `main-check.yml` runs when Forgejo's green run pushes `main` to GitHub: it builds and tests macOS and Windows (and builds Linux), so a macOS- or Windows-only failure shows at the check tag and not at the release. It also saves the Rust cache on `main`, which the release build restores (both use the cache key `release-<OS>`), so release builds stop starting cold (#452, #453, ADR-0025).
- `scripts/make-release.sh` stops when GitHub's latest main-check run is red or still running, or when `gh` cannot tell; `--ignore-github-check` skips this and `--dry-run` reports it (#453).
- Forgejo's `mirror` job pushes a release tag to GitHub before `main`, so the main-check of a release commit leaves the tests to the release build (#452).
- The GitHub release workflow no longer repeats checks that already ran on the same commit: clippy and `cargo fmt --check` (run by `scripts/land.sh` and `scripts/cross-check.sh` on every landing) and the Linux tests (run by Forgejo before the tag reaches GitHub) are gone from it, so a release build is faster. macOS and Windows are still tested there (#454).
- Forgejo CI: a release tag no longer rebuilds the TeX image without the cache (only the weekly run does) (ADR-0026, #455).
- Developers only: tests for the mail index rebuild guard, calendar edit and delete from the window, the updater's recovery when an install step fails, account switching and attachments in the compose window, clearing caches from the Data usage window, and clicking, dragging and zooming in the graph view. The coverage baseline is now 94.66 % (#463).

### Fixed

- In the editor, a click on a media player (play and pause, the seek bar, the volume, mute, the card, and the round buttons over a video) no longer takes the keyboard from the text: the cursor and the selection stay active, Space types a space, and you keep typing where you stopped. In the reading view a click still gives the player the keyboard, so Space toggles it (#441).
- LaTeX export: the red `[missing: …]` placeholder of a picture or embed is in the paper red of missing links (RGB 192, 28, 40), as the built-in PDF export prints them, instead of the screen colour (#315).
- Editor: when the 100 % button or a drag on the edge of a picture or video writes a width into its link, the text cursor and the selection move with the text, like with any edit before them. Before, a letter typed right afterwards landed a few characters away from where the cursor was (#443).
- Settings → Keybindings: a chord with Ctrl, Alt or Shift (such as Ctrl+Alt+1) is recorded as that one chord. Before, the modifier keys pressed first were recorded as chords of their own, so Enter found a sequence that starts with a bare Ctrl, refused it, and the action kept its old key (#445).
- A sequence that waits for a chord with Ctrl, Alt, Shift or Super (such as Space Ctrl+F) is no longer cancelled by the modifier key itself, which a keyboard reports before the key (#445).
- A tag whose real-server tests, coverage or unused-dependency check failed no longer reaches GitHub: the mirror job now waits for every check job (build, unused-deps, coverage, env-tests), so a red run stops a release too (#461).
- The real-server suites of the tag CI (`env-tests.sh run`) no longer fail in a fresh container where the Rust toolchain is not installed yet: one `cargo test --locked --no-run` runs first and installs it, so the suites that go at the same time do not install it together (#462).
- The mail index's hand-back after a rebuild no longer closes a mail store that was not being rebuilt. Not reachable from the app today; a guard for later callers (#464).
- Developers only: the test suite on Windows no longer fails in the tests of the podman stack lock. The lock (`src/env_lock.rs`) is built for Unix only, because the stacks run on Linux only and a Windows file lock is mandatory, so the token in the locked file could not be read; on other systems the ignored env tests take no lock (#465).
- Developers only: two test fixes that blocked the first check tag of 0.6.10. The AUR package test (BUILD-026) failed for a pre-release `Cargo.toml` version such as `0.6.10-rc.1`; it now compares the version by semver, a release ranking above a pre-release of the same X.Y.Z. The TeX env test's PDF stream reader dropped a stream's last byte when it was `\r`, so a German beamer test failed on some dates; it now reads a stream by its direct `/Length` (#459, #460).
- Developers only: `scripts/make-release.sh` and `scripts/make-tag.sh` no longer stop with "the changelog needs an update first" when `[Unreleased]` has entries. The check cut its `git show CHANGELOG.md` pipe short, and with the changelog bigger than the pipe buffer `git show` died of SIGPIPE, which `pipefail` read as "no entries"; the check now reads the whole file. `scripts/bench-tests-wanted.sh` no longer ends a `git diff` pipe early either (#466).

### Tests

- Tests for `main-check.yml` (triggers, same commands as `build.yml`, shared cache key, release gate and the order of the mirror pushes) in `tests/release_downloads.rs`, and for the GitHub check of `make-release.sh` in `scripts/test-release.sh`.

## [0.6.9] - 2026-11-19

### Added

- **The tests the spec names are checked** (#435). A test in `tests/spec_refs.rs` reads the Test column of every
  `IS NOW` row and fails when a named test, file or script is gone, or when a row names none and does not start
  with `process`, `CI` or `untested`. Rows that pointed at renamed or moved tests were corrected. Nothing changes
  in the app itself.

### Fixed

- Test only: `scripts/install.sh`, the installer in the Linux download, no longer uses `install -D` (it makes the folders with `mkdir -p`), so its tests also run on macOS, whose BSD `install` has no `-D` (#439).

## [0.6.8] - 2026-11-19

### Added

- **Dataflow diagrams recorded from real runs** (#409). The steps of a flow are marked in the code, and a test records a
  real run and draws it as a sequence diagram, so a diagram can no longer drift from what the code does. The first
  one is the IMAP mail sync, in `docs/dataflow/mail-sync.md`. Nothing changes in the app itself.
- **An index of the dataflow diagrams** (#410). `docs/dataflow/README.md` lists every recorded dataflow diagram with its
  doc, its driving test and how its drift is checked, and a test keeps the index and the diagrams in step. Nothing
  changes in the app itself.
- **Dataflow diagrams of the flows that talk to real servers** (#411): mail OAuth sign-in and token renewal, sending,
  flagging, moving and deleting, the IDLE watch, OpenPGP and S/MIME; CalDAV discovery, loading, editing and moving;
  TeX export, book export and setup. Each is recorded from a real run of its container test (`docs/dataflow/`), so a
  diagram can no longer drift from what the code does. Nothing changes in the app itself. A diagram now shows the steps of other flows that its code calls (the
  book export shows the TeX compile, the IDLE watch the folder check), and a CalDAV conflict has its own diagram.
- **Dataflow diagrams of the flows that need no containers** (#412). Git sync (first sync, pull, incremental, LFS
  upload and on-demand download), Mastodon and Bluesky sign-in and timelines, and PDF and book export now have
  diagrams in `docs/dataflow/`, recorded from real test runs, so they cannot drift from the code. Nothing changes in
  the app itself.
- **Dataflow diagrams of the import flows** (#413). The Word import, the Thunderbird store lookup, an imported e-mail and
  a drop from another program each have a sequence diagram in `docs/dataflow/`, recorded from a real run of a test
  that runs on every `cargo test`. Nothing changes in the app itself.
- **Dataflow diagrams of the vault internals** (#414). Opening a vault, the vault check, the search across the
  vault, the recovery of text that could not be saved, the data-usage scan and cleanup, and the link preview
  fetch each have a diagram recorded from a real test run, in `docs/dataflow/`. Nothing changes in the app itself.
- **Dataflow diagrams of the auto-update** (#425). The check for a new release, the download with its checksum and the
  install into a portable folder or through `install.sh` are marked in the code, and tests record real runs against a
  pretend GitHub and draw them in `docs/dataflow/update.md`. Nothing changes in the app itself.
- The colour buttons of the formatting toolbar have keys for both parts: Alt+Y (highlight) and Alt+F (font colour) apply the button's current colour, like a click on its coloured part; the new Alt+Shift+Y and Alt+Shift+F open the palette, like the ▾. While Alt is held the badges show `Y` on the coloured part and `⇧Y` on the ▾. In an open palette the digit keys pick: 1 to 8 the swatches (each shows its digit), 0 "No highlight" / "Default colour" (#431).

### Changed

- Alt+Y and Alt+F (actions `note.highlight_colour` and `note.font_colour`) no longer open the palette but apply the current colour; the palettes have their own actions, `note.highlight_palette` and `note.font_palette` (#431).
- Shale is now licensed under the GNU GPL, version 3 or later (before: MIT; releases up to 0.6.7 stay MIT) (#434).

### Fixed

- Importing the newest templates from CTAN no longer gives up when the mirror CTAN picked is down: it tries two other mirrors (three in all), and the error names every mirror tried (#428).
- Test only: the clock-card media test is timed from the card's own start and its next wakeup, so it no longer fails on a loaded macOS runner (#430).
- A click on the copy button of a code box, on a fold marker or badge, or on a player button no longer moves the text cursor of the editor while the button is held, and when the keyboard goes back to a selection the end that moves stays where it was (#432).

## [0.6.7] - 2026-11-19

### Added

- **Updates from GitHub** (#421, ADR-0023): Shale can look for a new release (a short while after it starts and every 24 hours), check the download against the release's SHA-256 file and, as you choose in **Settings → Updates**, only tell you (**Notify**), download it (**Download**) or install it (**Install**); **Off** makes no request at all, and so does `SHALE_UPDATES=off` in the environment. Until you choose, the first update found asks. The notice has What's new, Skip this version and Later; after an install, **Restart now** saves everything and starts the new version, and the old program is kept as `shale.old` until the new one has run. A Linux folder or an `install.sh` install, a macOS app and a Windows folder update themselves; a package-managed install (AUR `shale-bin`, `/usr/bin`) and a development build are never touched and only get a notice, and a folder you cannot write gets the command to run. The checksum shows a download arrived whole, not who made it (#422).

### Changed

- The TeX Live install and the update check share one download helper (`curl` with a size limit, timeout and cancel).
- Forgejo CI: `v*` tags (release and check tags) take TeX Live for the TeX image and the install test from the homelab mirror named by the Actions repository variable `SHALE_TEX_MIRROR` (never committed), as the public CTAN mirror answered the runner with 403; the weekly run keeps the public default (#424, #371).

### Fixed

- `scripts/aur-update.sh` no longer uses GNU-only `sed -i`, so it and its tests run on macOS too (#423).

## [0.6.6] - 2026-11-19

### Fixed

- **Tests that failed on the Windows and macOS builds** (#415): the Finder `PATH` logic splits on `:` on every platform, and the tests for the GnuPG plan text and for Ctrl+Home / Ctrl+End in the front matter fold use the platform's own path separator and command key. No change to the app.

## [0.6.5] - 2026-11-19

### Added

- **Dates on property cards open the calendar** (#358). The date of an e-mail card and the time of an event card
  are links in the reading view: a click shows that day in the calendar of the sidebar (or the calendar tab when the
  panel is not in a sidebar view).

### Fixed

- **The bench-compare script test no longer runs the benchmarks in a shallow CI checkout** (#404). A shallow clone has
  no commit older than the harness to refuse, so the test skips that part there.
- **The GitHub release page lists every version since the last published one** (#406). Versions that were tagged on
  Forgejo but never reached GitHub (a red run is not mirrored) are no longer left out of the next release page: it
  shows the notes of each of them under its own version heading, and the release check checks the same text, so the
  GitHub build no longer fails for issues that an unpublished version already described.
- **A release no longer breaks its own landing** (#406). `make-release.sh` handed the last published release to
  everything it started, so the tests that `land.sh` runs failed on it; it now reaches only the release check, and
  the script tests ignore a value from the environment.

## [0.6.4] - 2026-11-19

### Added

- **Centred, right-aligned and justified paragraphs** (#340). A paragraph or heading between a line
  `<div align="center">` and a line `</div>` (also `align="right"` and `align="justify"`, the HTML that
  Obsidian shows too) is aligned that way in the reading view, the PDF export and the LaTeX export
  (`center` and `flushright`); the editor shows the two lines dimmed and the outline still lists a
  heading inside. Three new toolbar buttons (left, centre, right) put the paragraphs under the cursor
  or the selection into a wrapper, change it or take it off. The user guide's PDF note describes it.
- **Performance benchmarks** (#343). A build with the cargo feature `bench` (never a shipped build)
  runs `shale --bench <file.json>`: headless scenarios for opening and checking a vault, search, the
  link index, the graph, frames of the window, Word import, PDF export, mail, calendars, social
  feeds, git and git-LFS sync, and a worst case with everything loaded at once, each with time,
  allocations and peak memory, and flamegraphs through `scripts/bench.sh --flamegraph`.
  `scripts/bench-compare.sh` compares two commits (exit code 1 on a regression) from several
  interleaved runs of each, so a busy computer does not make false alarms: a time counts only
  beyond the spread of the runs, allocation counts and peak memory count on their own, and a scenario
  whose definition changed is marked `changed` instead of judged. It writes `compare.json` for an
  agent (with an agent playbook in `docs/perf/README.md`) and, with `--flamegraphs`, flamegraphs of
  both commits and their difference for what got slower. Every release stores its results in
  `docs/perf/results/`, compares them with the last release and stops on a regression
  (`--allow-perf-regression`, `--skip-bench`). `docs/perf/README.md` has the baseline and the weak
  spots it found (BUILD-028 to BUILD-038).
- **Folds are remembered across restarts** (#364). The folded blocks of a note come back the next time you
  start Shale, in the editor and in the reading view. They are kept per note in the vault's `.shale/state.json`,
  never in the note. A fold whose block you deleted is forgotten, a deleted note loses its entry, and a renamed
  or moved note keeps its folds.
- **Light and dark themes** (#401). Settings → Appearance has a **Theme** list: Shale (as before), Dracula,
  Nord, Catppuccin Mocha and Gruvbox Dark, and the light Catppuccin Latte, Solarized Light, GitHub
  Light and Gruvbox Light; a change applies at once. **Follow the system's light or dark mode** takes
  a light and a dark theme and switches with the system. Themes are Alacritty terminal colour
  files, read as they are: put any `.toml` of the alacritty-theme collection into the themes folder
  (Open themes folder, Reload themes) and it is listed by its file name; an optional `[shale]` table sets
  any colour exactly, and a broken file is skipped with its error listed (CFG-062 to CFG-065).

### Changed

- **Word import keeps the alignment of paragraphs** (#340). A paragraph or heading that is centred, flush
  right or justified in Word (directly or by its style) while the body text is not is written between
  `<div align="…">` lines; a left-aligned paragraph in a justified document gets `align="left"`.
  Notes imported earlier keep what they have until they are imported again.
- **A big vault is much faster** (#343). The benchmarks found the slow places and each one is fixed below. The
  timings come from the benchmark harness on large test vaults and are approximate; your computer will differ.
  The before and after of every area is in `docs/perf/README.md`.
- **Finding a file in the vault is a lookup** (#378). A file is found through a hash instead of a search, so
  a rescan of a large vault went from about 513 ms to 42 ms.
- **Backlinks, unlinked mentions, tasks and the sidebar search are kept** (#379). They are worked out once for
  a state of the vault and not again in every frame: an idle frame of a large vault went from about 60 ms to
  1.4 ms, and the backlinks view from 372 ms to 34 ms.
- **The task view draws only the rows in view** (#377). Rows far from the view are not laid out, so a
  task-view frame of a large vault went from about 727 ms (211 ms after the #379 change) to 2.9 ms.
- **The calendar remembers the occurrences it shows** (#380). The occurrences of a range are computed once
  until the events change; a calendar frame went from about 16 ms to 2 ms.
- **The social feed lays out only the posts near the view** (#381). Posts far from the view keep their space
  without being laid out; a feed frame went from about 46 ms to 0.9 ms.
- **The graph of a big vault moves faster** (#382). Above 300 notes the graph uses a quadtree for the
  repulsion and a time budget for its physics, and a graph frame went from about 373 ms to 30 ms. A graph of
  fewer than 300 notes is laid out as before.
- **A big vault opens faster** (#384). Its notes are parsed on several threads when it opens; opening went
  from about 140 ms to 74 ms.
- **The PDF export compresses its fonts once** (#386). The bundled fonts are compressed once per run
  and not once per document; exporting a long note went from about 113 ms to 98 ms. The compression
  level stays at 6, because levels 3 and 1 were measured and saved little time for larger files.
- **Every section heading in Settings and the dialogs has the same room above it** (#357). The mail accounts,
  LaTeX settings and setup, calendars, social accounts, data usage and import summary dialogs now set their
  sections apart by 24 points like the Settings pages, except the first heading at the top of a page, and the
  small hand-made gaps between related items are gone.
- **Saved posts write their header once, too** (#361). A note from a post or a thread has the
  author, date, link and the rest in its properties only, then goes straight to the text, as
  e-mails and events do. What the old list held and had no property gets one: `author-url`,
  `reposted-by` and `people`. The box **Header list in the note body** (Import dialog, Settings →
  Import) now also brings the Author / Posted / Source list back for posts. Notes saved before stay
  as they are.
- **Landing can take TeX Live from a local mirror** (#371, developers only). `scripts/land.sh` reads
  `SHALE_TEX_MIRROR` or the file `land/tex-mirror` in the git folder (never committed) and passes the
  mirror to the TeX Live install test and to the TeX test image; without a setting, and in CI, the public
  default is used.
- **Typing in a very large note is quicker** (#375). The editor formats the text of a note block by block
  (blocks of whole lines between blank lines, never cut inside a code fence, the front matter, a formula
  or a link) and keeps each block's format under its text, so a typed key formats the one block it is in
  and a cursor move to another line (Markdown preview) formats the two blocks it leaves and enters, instead
  of the whole note. On a note of 20 000 lines this took 25 ms of a key's 60 to 70 ms; the result is the
  same as before. Ordinary notes are not affected.
- **PDF paper look: tables like Word's** (#388, #389). In the plain paper look a table spans the full text width and has horizontal rules only (above, under the header, between rows, at the bottom), no vertical lines. The export dialog's "From the note" line now names the paper look and a note's heading weight and sizes.
- **The dev build keeps line tables only** (#390). The `dev` profile and its dependencies are built with
  `debug = "line-tables-only"` (BUILD-027): backtraces still show function names and `file:line`, and a
  worktree's `target/debug` becomes much smaller and links faster. A debugger no longer sees locals and
  types; `CARGO_PROFILE_DEV_DEBUG=true cargo build` brings them back for a debugging session.
- **The widths of the sidebars are kept** (#393). The widths the left and the right sidebar were dragged to come back after a restart (`left_sidebar_width`, `right_sidebar_width` in the settings). "Reset layout" in the ribbon's menu also resets them.
- **The colours of the window come from a palette** (#401). Every colour is read from the palette of the
  current theme instead of from constants (CFG-061); Shale's own palette has exactly the colours of
  before, so nothing looks different. The PDF export always prints with Shale's palette.

### Fixed

- **TeX Live install and update repair a font that cannot be loaded** (#362, #327). After an install or update Shale builds
  the font list of TeX once and compiles the test document; if a font cannot be loaded (the install once stopped at
  `lmroman10-regular`), it installs the font packages again from another mirror, rebuilds the list and tries once more.
  The message that ends an install or update names the mirror it came from, and a warning names it too. A compile that
  stops at such a font says in plain words that the fonts or the font list of the TeX look incomplete.
- **A mirror that hands out damaged packages is left for another one** (#362, #327). `tlmgr` skips packages whose checksums
  differ (a mirror in the middle of a sync) and still reports success; the install, the update, the font repair and the
  test image now notice that, try a fresh mirror (three runs at most) and otherwise say which packages and mirrors failed.
- **Mail, Calendar, Tasks, Graph and Settings-file tabs come back after a restart** (#387). They were
  dropped; now every tab returns in its pane and position, pinned as before. The mail tab comes back on
  its folder and open message (the first folder when they are gone), the calendar on its mode and day,
  Tasks with its filter and the graph with its switches. Nothing is asked when they come back.
- **The tag CI is green again on its Linux image** (#391). Three tests depended on the machine: one
  expected the local hour of an imported mail (the CI container runs in UTC), two expected GnuPG to
  refuse an unknown option in `gpg-agent.conf` (GnuPG 2.2 of the CI image accepts it). They no longer
  depend on the timezone or the GnuPG version. No change for users.
- **The media tests no longer fail now and then under load** (#392). The tests for a damaged video, a missing
  ffmpeg and the seek in the progress bar wait for their condition instead of a fixed pause. This changes
  tests only, not the app.

## [0.6.3] - 2026-11-19

### Added

- **A note says how its PDF is printed** (#339). The properties `print-paper` (`A4`, `A5`, `Letter`,
  `Legal` or `<W>x<H>mm`), `print-margins` (four numbers in mm), `print-font` (`serif`, `sans`,
  `mono`), `print-font-size`, `print-line-spacing`, `print-paragraph-spacing`, `print-align` (`left`
  or `justify`), `print-heading-color` (`#RRGGBB`), `print-title` and `print-toc` are read by the PDF
  export; each is optional and a bad value is ignored. The export dialog opens with the note's paper,
  margins, text size, title and contents filled in; the other keys are read from the note when you
  export. The serif font is the bundled Liberation Serif (Times-compatible). The user guide lists
  every key in the PDF note.
- **A department logo beside the university's** (#341). Departments and institutions (the HRZ, the
  faculties, …) have a logo of their own, and documents show it with the TU logo. Every template set has
  an optional second logo slot: **Choose department logo…** and **Remove department logo** in Settings →
  LaTeX → Template sets, a drop while that form is open, and a second field "Department logo" on the logo
  step of the guided setup (a dropped file is the university's logo while that is missing, else the
  department's, and a message says which). Documents show it **below** the TU logo, right-aligned and as
  high, as the TU's rule for a second logo says: publication, report, thesis, doctoral thesis, leaflet,
  poster, scientific poster, slides and letter. Exercise sheets and announcements have no room for it and
  do not show it. The export dialog has a **Department logo** box when the set has one (on by default,
  greyed out for those two kinds); the test page shows it too. A kind says where its class puts the logo
  with the new `department_logo` key of its manifest.
- **Properties as a card in the reading view** (#351). The front matter of a note is no longer a wall of
  YAML: an imported mail opens with a mail header (sender, date, to / cc, subject, tag chips), an event
  with its time, place, attendees and a repeats badge, a Word import with one chip for its page
  (`A4 · 2.5 cm margins · 11 pt`), any other note with key and value rows. Ids (`message-id`,
  `event-uid`, `source`) and the `print-*` settings are left out; a ▸ toggle shows every property. A
  person with a contact note is a link. New setting **Properties in the editor / in the reading view**
  (Settings → Appearance: folded, card or raw; folded in the editor and card in the reading view by
  default). In the editor a note opens with its properties folded to one summary line (`▸ Properties · email · Anna Müller · …`); the cursor moving into it opens the raw YAML and folds it again when it leaves, and the setting `Raw` leaves it open.
- **Option "Header list in the note body"** in the Import dialog and Settings → Import (off by default)
  brings the list back for programs that do not show properties (#352).
- **Mail notes fold their noise** (#353). An imported mail (`type: email`) opens with its quoted replies
  (`… 12 quoted lines`) and its signature folded; a line `-- ` up to the next heading or the end is a
  foldable signature in any note. Click a badge to read the text; the folds are never written into the note.
- **A summary after an import of two or more items** (#354). The window "Import summary" lists the new
  notes, each with its name, a line about it (an e-mail: sender and date; an event: time and place;
  other notes: their first line) and an **Open** button, the files that were saved in the media folder
  and anything that went wrong. **Open all in tabs** opens every new note in a tab of its own. One item
  is opened as before.
- **Fold quotes, lists and heading sections in the editor** (#355). A heading section, a quote, a list
  item and a whole list can be folded: the block shows its first line and a muted `… n lines` badge, the
  rest is hidden (no line numbers for it), with the Markdown preview on and off. A ▾ / ▸ marker left of
  the first line (shown on hover, always for a folded block) and the badge fold and unfold with the mouse;
  **Ctrl+Shift+K** folds, **Ctrl+Shift+J** unfolds, **Ctrl+Shift+O** / **Ctrl+Shift+I** fold or unfold all
  (Settings → Keybindings → Note view), and vim-style editing has `za` `zc` `zo` `zM` `zR`. The cursor never
  stays in hidden text (moving into a fold, a link or a search hit opens it). Folds are kept for the session,
  never written into the note.
- **Code blocks as boxes** (#356). With the Markdown preview on, a fenced code block is a rounded box on the
  code background with a thin border, the ``` lines hidden (they show again when the cursor is in the block)
  and a small badge at its top right with the language (`code` when there is none). The reading view draws
  the same box. A code block folds like a quote or a list (`… n lines`, the marker, **Ctrl+Shift+K**, vim's
  `za`), also with the preview off.
- **Page breaks in the PDF and TeX export** (#359). A line `<div style="page-break-after: always;"></div>`
  (the form Obsidian writes; `page-break-before: always`, `break-after: page` and `break-before: page`
  work too) starts a new page in the built-in PDF export and writes `\newpage` in the LaTeX export, with
  no empty page at the top of a page or at the end of the note. The editor's Markdown preview and the
  reading view show it as a dashed rule labelled "Page break"; the new toolbar button "Page break"
  (Alt+J) inserts the line.
- **`print-break-before`** (#359): a note property with a heading level from 1 to 6; every heading up to
  that level starts a new page in the PDF and in the LaTeX export of an article or book. The export
  dialog has a box "Start each # heading on a new page", filled from the property, with the level next to it.
- **Word import keeps page breaks** (#360). A manual page break, a paragraph with "page break before"
  and a section break that starts a new page become a line `<div style="page-break-after: always;"></div>`
  in the note, so the PDF export breaks the pages where Word did. When Heading 1 (or Heading 2 …) starts
  a page in Word, the note gets the property `print-break-before: <level>` instead of a line before each
  heading.
- **Keep the signing key unlocked** (#363). Accounts…, Encryption shows how long GnuPG keeps a passphrase
  (10 minutes after the last use and 2 hours at the latest, unless set otherwise) and, with "Ask for the
  passphrase again after" (8 hours, 1 day, 1 week) and "Change with gpgconf…", can make that longer through
  GnuPG's own `gpgconf --change-options gpg-agent`. It shows the file and the two lines first and writes only
  after "Write with gpgconf". The same lines and the reload command are shown for doing it by hand, with the
  keyring tip of your system ("Save in password manager" on Linux, "Save in Keychain" on macOS). Shale still
  never handles a passphrase.
- **Fold in the reading view** (#364). The reading view folds heading sections, quotes, list items and lists,
  code blocks and signatures like the editor, with the same folds: fold a block in one and it is folded in the
  other (while Shale runs; never written into the note). A ▾ / ▸ marker left of the first line (on hover, always
  for a folded block) and the `… n lines` badge under a folded block fold and unfold with a click; **Ctrl+Shift+O**
  and **Ctrl+Shift+I** fold and unfold all there. A footnote jump or a click on a heading in the outline opens a
  fold that hides its target; tables, formulas and embeds inside a fold are not drawn. The properties card keeps
  its own toggle. The timing of the block finder on a large note is measured by an ignored test
  (`large_note_timing`).
- **A copy button on code blocks** (#365). A code box has a small copy icon at its top right, left of the language
  badge, in the editor (Markdown preview on) and in the reading view: it shows while you point at the box (in
  the editor also while the cursor is in the block, and always on a folded block's row). A click copies the code
  inside the block, without the fences and without a newline at the end, so a pasted command does not run at
  once; the icon turns into a tick and "Copied" for a moment. It does not move the cursor or unfold the block.
  The key **Ctrl+Shift+C** (`note.copy_code`, Keybindings, Note view) copies the block at the cursor.
- **A plain paper look for the PDF of a note with `print-*` properties** (#367). A note with any of the
  `print-*` properties is exported like a document on paper: list bullets and numbers in the text
  colour, a quote indented without a bar, inline code in the mono font without a dark box, code blocks
  on light grey with dark text and no syntax colours, and tables with thin black lines and a bold
  header without a fill. New properties `print-heading-weight` (`normal` or `bold`) and
  `print-heading-sizes` (six sizes in pt for the heading levels, such as `"16 13 12 11 11 11"`). Notes
  without `print-*` properties print as before.
- **Landing and development metrics for Prometheus and Grafana** (#368). `scripts/land.sh` now records
  every landing (result, failing step, duration of each step, load) in the git folder, and
  `scripts/dev-metrics.sh` writes them, with the build processes, agents, GitHub issues, releases and
  the Forgejo run of each tag, as a text file for node_exporter's textfile collector. A systemd user
  timer and a setup guide are in `tools/dev-metrics/` and `docs/dev-metrics.md`.

### Changed

- **Word import drops the table of contents** (#346). A Word table of contents (its title and the run of
  entry lines such as `Einleitung 2`) is no longer imported as plain paragraphs, because the PDF and
  LaTeX exports make their own contents page and showed both. Notes imported earlier keep the lines
  until they are imported again.
- **Word import keeps more text colours** (#347). Colours that come from a character style, a paragraph
  style (also through the styles they are based on) or a table style become `<span style="color: …">`
  like direct colours, and theme colours (`accent1`, `text2`, …) with their tint and shade are resolved
  to their hex value, also for `print-heading-color`. The colour of the body text, of headings (Heading
  1 to 9, Title, Subtitle) and of links is not tagged.
- **The colour buttons of the toolbar are split buttons** (#350). A click on the highlight or font
  colour applies the colour it shows at once (neon yellow and red at start, then the last colour
  picked); the ▾ beside it opens the palette as before. Alt+Y and Alt+F still open the palette.
- **Imported e-mails and events write their header once** (#352). The sender, recipients, date, place,
  organizer and the rest are in the note's properties only; below them the note goes straight to the
  title, the attachments and the message (an event: the title and its notes), instead of repeating
  them as a list of `**From:**`, `**To:**`, `**Date:**` lines. What the list held and had no property
  gets one: `people`, `original` and an unreadable `date` for e-mails; `repeats`, `organizer`,
  `attendees`, `status`, `url`, `people` and `original` for events. Notes imported before stay as they are.
- **Imported mail text is tidied** (#353). What an HTML mail leaves behind is cleaned on import: three or
  more blank lines in a row become one, trailing spaces go, and a long tracking link (a very long address
  with a query string) becomes a short Markdown link showing the host, `[example.org](https://…)`. Short
  links and links that are Markdown already stay.
- **Pictures among a mail's attachments are small embeds** (`![[file.jpg|200]]`, 200 points wide) instead of
  full-size embeds; other attachments stay links.
- **The signature stays recognisable.** It starts at a line that is exactly `-- ` (a bare `--` is made
  into that, with a blank line before it), and a quoted reply stays a run of `>` lines, so the editor can
  fold both (#353).
- **More room between the lines in Settings and the dialogs** (#357). Wrapped text has 4 points more between its
  lines, a new setting, paragraph or step is set apart by 8 points (more than the gap inside one item), and a new
  section of Settings by 24 points. The spacing is on a 4-point raster and applies to Settings and every dialog; the
  sidebar, the editor and the main window keep their spacing.
- **Word import: closer to Word** (#366). Highlights are written in Word's real colour, yellow too
  (`<mark style="background: #FFFF00;">`), not as `==text==`, which prints a paler yellow. The note's
  properties now also hold `print-heading-weight` (`normal` or `bold`) and `print-heading-sizes` (the pt
  of Heading 1 to 6) from the heading styles, and `numbered-headings: false` unless Word numbers
  Heading 1, so headings are not numbered where Word does not number them.
- **Folding is cheap on very large notes** (#369). Finding the blocks of a 20 000-line note took 20 ms on every
  frame (mostly counting equal titles) and is now 1 ms, and is skipped while the text is unchanged;
  looking up many folded blocks is linear. The reading view of a note of 2 000 lines or more draws only what is near the
  visible part (about 8 ms per frame instead of 120 ms for 20 000 lines); jumps to footnotes and headings still work.
- A folded code block in the reading view is one row: the `… n lines` badge sits in its box next to the language
  badge instead of under an empty box.
- **Faster real-server checks before a landing** (#370). The LuaLaTeX suite (`tex_env`) of
  `scripts/land.sh` runs with four test threads instead of one (about 36 minutes before);
  `SHALE_ENV_TESTS_TEX_THREADS` sets the number. The CalDAV, mail, install and import suites stay at one thread.
- **The editor is fast on very large notes** (#372). A frame of a 20 000-line note took 180 to 260 ms and
  now takes a few milliseconds while nothing changes (the cursor blinking, the pointer moving): the laid-out note, the
  tables and footnotes, the counts of the status bar and the places of the fold markers, code boxes and heading
  numbers are kept until the text, the line the cursor is in (in the Markdown preview) or the view changes. Typing
  a key takes about 60 to 70 ms on such a note (before: 250 ms or more); ordinary notes are not affected.
- **The macOS download is a dmg** (#373). `shale-<version>-macos-arm64.dmg` opens to `Shale.app` and an
  `Applications` link to drag it onto, with a full-size icon (a 1024 pixel picture was added to the set) and a bundle that is signed ad hoc, so macOS offers
  "Open Anyway" (right-click, Open) instead of calling it damaged; it is still not notarised, and the
  Keychain may ask again after an update. An app started from Finder or the Dock now also finds the
  programs Homebrew installs (`git-lfs`, `ffmpeg`, `gpg`, `pandoc`): `/opt/homebrew/bin`,
  `/opt/homebrew/sbin` and `/usr/local/bin` are added to its search path.
- **The editor comes back as you left it** (#374). After a restart each note opens with its cursor or
  selection and scrolled where it was, in every pane, and a note in the reading view stays in it. The
  current colours of the toolbar's highlight and font colour buttons are remembered in the vault too.
  The session state is written every few seconds when something changed, and at exit.

### Fixed

- **Default keys that the system or the desktop takes for itself** (#345). No default key is one
  that macOS, Windows or the Linux desktops reserve (KEY-019). On Linux and Windows: previous / next
  tab are **Ctrl+PageUp / Ctrl+PageDown** (were Ctrl+Alt+H / L), next / previous pane **F6 /
  Shift+F6** (were Ctrl+Alt+→ / ←, which switch workspaces on most desktops) and close pane
  **Ctrl+Shift+W** (was Ctrl+Alt+W, which closes a pane on Hungarian and Czech keyboards where
  AltGr+W is `|`). On macOS the keys that move the keyboard one place left / right are **Cmd+Shift+H / Cmd+Shift+L**
  (Cmd+H hides the app), back / forward **Cmd+[ / Cmd+]** (Option+← / → move by word) and previous /
  next tab **Cmd+Opt+← / Cmd+Opt+→** (Cmd+Opt+H hides the other apps). The Alt keys of the open mail's
  buttons have no default on macOS, like those of the note buttons. A key you set yourself is not
  refused for being reserved, and a key you changed keeps working. The Back and Forward buttons show
  the key that is set.
- **A release or check tag can no longer lose its notes to a landing that comes meanwhile** (#348).
  `scripts/make-release.sh` and `scripts/make-tag.sh` hold the landing lock from before they read
  `origin/main` until the tag is pushed, so a branch landed by someone else waits and lands after
  the tag (FORGE-011, FORGE-013, FORGE-014). Before tagging they run the release check on the landed
  merge and, for a release, require `docs/changelog-pending/` to be empty there; if that fails nothing
  is tagged, the missing issue is named, and running the script again for the same version, after the
  entry landed, tags `origin/main`.

## [0.6.2] - 2026-11-18

### Added

- **Word import keeps the look** (#338). A Word file's page setup, body font family and size, line and
  paragraph spacing, alignment and the colour of Heading 1 are written into the note's properties
  (`print-paper`, `print-margins`, `print-font`, `print-font-size`, `print-line-spacing`,
  `print-paragraph-spacing`, `print-align`, `print-heading-color`, `print-title`, `print-toc`), so the
  PDF export looks like the Word document. Text colours and highlight colours (other than yellow)
  become `<span style="color: …">` and `<mark style="background: …">`. Centred and right-aligned
  paragraphs are not kept yet (#340).

### Changed

- **A Word import writes a properties block every time**, not only when the original is kept
  (`source` is still there only with the original). A Word file's Title and Subtitle now become a
  `#` heading and an italic line instead of being dropped.

## [0.6.1] - 2026-11-18

### Added

- **Install script for Linux** (#330). The Linux download now has `shale/install.sh`, which works on
  any distribution: `./install.sh` installs the program to `~/.local/bin` and the app menu entry and
  icons (all sizes) to `~/.local/share`, `--prefix /usr/local` installs for all users,
  `--uninstall` removes exactly what it installed. It runs again to upgrade, warns when the program's
  folder is not on `PATH` and names missing libraries with the package names for Arch, Debian and
  Fedora (BUILD-025). There is no AppImage (it needs FUSE 2 and has no menu entry) and no Flatpak
  (its sandbox would block vault folders and the programs Shale starts).
- **Arch Linux package** (#337). `packaging/aur/shale-bin/` has a PKGBUILD for the AUR package
  `shale-bin`, built from the Linux download of a release: the program, the app menu entry, the
  icons, the licence and the documentation are installed system-wide, and the optional programs
  (git, git-lfs, curl, ffmpeg, gnupg, pandoc) are named as optional dependencies. `scripts/aur-update.sh`
  moves it to a new release (version, checksum, `.SRCINFO`) (BUILD-026).

### Security

- **lettre updated to 0.11.24** (#331 to #336, RUSTSEC-2026-0560 to 0565): STARTTLS response
  injection (#331), missing dot-stuffing after a bare CR or LF (#332), a panic on a white-space EHLO
  line (#333), a quoted local part sent to a different address (#334), a panic on a CR or LF in a
  display name (#335) and unbounded buffering of SMTP replies (#336). Shale sends through its own
  SMTP client, which already refuses bare line breaks and dot-stuffs, so only the message builder
  fixes (#334, #335) touched Shale's path; both are now covered by tests (MAIL-125).

## [0.6.0] - 2026-11-18

### Added

- **Downloads for macOS and Windows** (#71). Every release page now has `shale-<version>-macos-arm64.tar.gz`
  (Apple silicon) and `shale-<version>-windows-x86_64.zip` next to the Linux tarball, built, tested and
  packed the same way (the program, the licence, the README, the changelog, the icon and the docs with the
  user guide), each with a `.sha256` file and covered by the release's build attestation. They are not
  signed or notarised: the release notes and the README say how to start them past macOS Gatekeeper and
  Windows SmartScreen. The release is published only when the Linux, macOS and Windows builds all passed
  (BUILD-024).

## [0.5.1] - 2026-11-18

### Added

- **A folder as one PDF through a LaTeX template** (#309). The export dialog of a folder has the same
  engine choice as a note's: with **LaTeX template** the folder's book is written as one document of a
  kind of the article or book family (publication, report, exercise, thesis, PhD thesis; slides, letters
  and posters are greyed with the reason). In a thesis the subfolders become `\part`s and the notes
  `\chapter`s, in an article kind the same starts at `\section`; the folder notes' text goes under their
  part and the top folder note's before the first chapter; the fields start with the top folder note's
  properties; links between the chapters go to their chapter or heading (`\hyperlink`); pictures of the
  same name in different folders stay apart. LaTeX numbers the parts and chapters and makes the contents
  and the bookmarks (BOOK-013 to BOOK-017).

### Fixed

- **Installing or updating Shale's TeX Live no longer fails on an older CTAN mirror.** The default
  download address sends each request to some CTAN mirror, and the installer and the package step could
  end up on two mirrors of different age; the package manager then refused the older one ("seems to be
  older than the local installation"). An install or update now asks once which mirror to use and takes
  everything from that one mirror, and if that mirror is behind your installation it tries another one,
  three times at most (#320).
- **Windows: new notes and renames with a path over 260 characters work.** A vault deep in your profile
  folder plus a long note name (up to 200 bytes) went over Windows' old path limit, and creating or
  renaming such a note failed with "the system cannot find the path specified" (#321).
- **macOS and Windows: a rename that only changes letter case works** (`plan` → `Plan`). Before, it was
  refused as "a file with that name already exists", because these systems treat both spellings as the
  same file (#321).
- **Names check: a numbered name stays within 200 bytes.** When the portable name the check proposes is
  taken, the number now replaces the end of a long name instead of making it too long again (#321).

### Internal

- **macOS: the closed-port test helper is refused again instead of timing out.** The helper for
  tests of an unreachable server held a bound socket that does not listen on every Unix. Linux
  answers a connect to it with a refusal, but macOS drops it, so the connect timed out and two
  tests failed on the v0.5.0 tag. On macOS the helper now holds the port of a connected socket,
  which is refused and which a plain bind does not get; Linux and Windows keep the bound socket,
  which no bind gets at all. The comment of `ClosedPort` explains the rules of each system (#302).
- **The TeX test image uses one mirror too**: `tests/tex-env/install-texlive.sh` installs TeX Live in one
  step from one resolved mirror, retries `tlmgr` on a fresh mirror when its mirror is older, and records
  the mirror in `/etc/shale-tex-build`. This fixes the red env-tests job of the 0.4.1 tag (#320).
- **The names-check tests run on macOS and Windows**: the check of a folder works on a list of names, so
  names those systems cannot store (`a:b.md`, `Plan.md` next to `plan.md`) are tested everywhere; the
  parts that need such files on disk find out at run time whether the file system holds them and are
  skipped with a note where it does not (#321).
- **The test of "Update set" runs on Windows too** (#323). It wrote the folder it imports from into
  `set.toml` by hand, and a Windows path's backslashes are escapes in a TOML string, so the set could
  not be read and had no **Update set** button. The test now writes `set.toml` the way an import does,
  and a new test checks that a Windows path of a local source is read back unchanged. Sets imported on
  Windows were not affected: the import always wrote the path escaped.
- **Tag runs of the `env-tests` job are faster** (#325). Refreshing the TeX Live image, the real
  TeX Live install and the template import now run only in the weekly scheduled run, not on every tag
  (ADR-0021); a tag run builds the TeX image from its cache and runs the CalDAV, mail and TeX
  compile tests.
- **A mail UI test no longer races on slow machines** (#326). The test for a mail that may have
  been sent waited for the Outbox entry to read "may have been sent" and then expected the notice
  at once, but the entry is re-read from disk before the send job's answer adds the notice; on the
  slower Windows runner the check came in between. The test now waits for both. Shale itself
  was not affected: the mail was classified correctly.
- **Release tags and the weekly run check the current TeX Live; check tags do not** (#325). The
  refresh of the TeX Live image, the real TeX Live install and the template import run only in the
  weekly run and for release tags (`vX.Y.Z`), not for check tags (`vX.Y.Z-rc.N`, ADR-0021), which
  build the TeX image from its cache and run the CalDAV, mail and TeX compile tests. This replaces
  the earlier note that only the weekly run did.
- **Tag runs of the `env-tests` job reuse the TeX image** (#325). Podman's storage of the job is kept on
  a volume of the homelab runner (ADR-0022), so a check tag run no longer builds the TeX Live image and pulls
  the CalDAV and mail images again; unused images are pruned after every run.

### Tests

- **The folder-note test of the PDF book passes on Windows** (#322). It compared the folder note's path
  as OS text (`T\A\Index.md` on Windows) with a vault path; it now compares it as a vault path with `/`
  (`vault::rel_to_string`) and as the vault's own path. The book itself was not affected: it only uses
  the vault's paths and compares them as paths, never as text.

## [0.5.0] - 2026-11-17

### Added

- **Template sets for the LaTeX export.** Settings → LaTeX → Template sets imports the templates a PDF
  is made with into your vault (`templates/tex/<set>/`), so they are synced and backed up with your
  notes. **Import TU Darmstadt templates** downloads the tested tuda-ci 4.06 (its checksum is checked),
  makes the class files with docstrip and sets up the 11 document kinds (publication, report, exercise
  sheet, thesis, doctoral thesis, slides, letter, poster, scientific poster, leaflet, announcement), each a
  small `kinds/<kind>.toml` you can edit. The newest version from CTAN, a GitHub repository, a `.zip`,
  a `.tar.gz` or a folder can be imported as well (a set without Shale's manifests is imported without
  kinds, which are added by hand); archives are read by Shale itself and anything that could write
  outside the set's folder (`..`, absolute paths, links) refuses the whole archive. Importing is a job with
  progress and Cancel and never leaves half a set in the vault; **Update set** keeps your `settings.toml`
  and logo and moves the old files to the vault's trash; **Remove set…** moves a set to the trash.
  **Test page** compiles a small document of the set with your TeX and opens the PDF. The university's logo
  may not be redistributed: **Choose logo…** copies yours into the set, and until then compiles use a
  placeholder and the set says "logo missing: exports are not CI-conformant". The export dialog and the
  conversion of notes follow with the next steps (TEX-005 to TEX-007, TEX-024 to TEX-027, #280, #277).
- **Export a note to PDF through a LaTeX template** (#281). The export dialog of a note has an engine
  choice at the top: **Built-in** (the default, as before) or **LaTeX template**. With the template you pick
  a template set of your vault and a kind (publication, report, exercise sheet, thesis, doctoral thesis,
  slides, letter, poster, scientific poster, leaflet, announcement), fill the kind's fields (they start
  with the note's properties: `author`, `subtitle`, `reviewer`, `recipient`, `subject`, … and a note's
  name is its title), switch the table of contents (for the kinds that have one) and press Export:
  Shale writes the note as a LaTeX document and compiles it with LuaLaTeX in the background, with
  **Cancel**. Headings follow how they are nested starting with `#`: sections for a publication,
  chapters for a thesis, a section and slides for slides (`#` a section, `##` a slide), bold lines in a
  letter, boxes on a scientific poster; the numbers follow the note's setting and the contents and
  the PDF bookmarks list every level. Emphasis, strike-through, inline code, fenced code in its syntax
  colours, quotes, lists, task lists, tables (alignment, header, the column widths you set), web links,
  the colours and highlights of the editor, pictures sized as in the editor, drawings as vector
  graphics and PDF pages are written, and every special character of a note stays text: a note
  cannot run a command or end the document. A file that is missing is a visible line instead of a failed
  compile. When LaTeX fails, the dialog shows the file and line or the missing package, and **Save the
  .tex** writes `main.tex` and its pictures into a folder next to the PDF to debug it with your own TeX.
  The dialog remembers the last engine, set and kind in the vault, and a note can name its template
  with `latex-template: tuda/thesis`. The 11 kinds of the TU Darmstadt set are tested in German and
  English against a real LuaLaTeX (TEX-008 to TEX-016, TEX-018, TEX-019, #281, #277).
- The test image and the managed TeX Live install `ulem` (strike-through) too.
- **A guided setup for the LaTeX export** (#282): **Settings → LaTeX → Set up LaTeX export…**, and by itself
  the first time you choose **LaTeX template** in the export dialog of a vault where no TeX is usable or no
  template set is there (the hints of that dialog for a missing TeX, template set or logo have a **Set up…**
  button as well). It is for people who are not firm in TeX but want to use their university's templates, and
  takes five steps with a step indicator, Back, Next and Cancel, a green tick on every step that is done
  already: **TeX** (found, or installed after you agree: about 100 MB to download and 330 MB on disk, with
  progress and Cancel; the built-in export needs none of it), **Templates** (the tested TU Darmstadt version,
  the newest from CTAN, a file, a folder or GitHub, or a set the vault has), **Logo** (the university's logo
  may not be shipped with Shale: the step says where members of the university get it and takes the file by
  path or by **drag and drop onto the window**; Later skips it and exports carry a placeholder), **Your
  details** (the accent colour as swatches with their codes `0a` to `11d`, the language, your department and,
  if you like, your name, written to the set's `settings.toml` and kept by an update) and a **Test page** that
  is made with your templates and details and opened, or, when it fails, explained in words with **Save the
  .tex** and **Run again**. The setup opens by itself once per vault and returns to the export dialog (on the
  LaTeX engine, with the set chosen) or to Settings.
- **Drag and drop onto the setup and onto Settings → LaTeX**: a PDF or PNG dropped while the setup is on its
  logo step becomes the set's logo, a `.zip`, a `.tar.gz` or a folder dropped on the templates step is
  imported as a template set, and the same works on Settings → LaTeX (an archive or folder is imported, a logo
  goes to the set whose **Choose logo…** is open or to the only set that has a logo slot). These files are no
  longer offered to the Import dialog as notes or media (TEX-017, TEX-028).
- A set's `settings.toml` has an `author` (the guided setup asks for it): it fills the author field of the
  export dialog, and the document, when a note gives none; the `department` does the same for the kinds that
  have one (TEX-018).
- **Word files become notes.** With **Settings → Import → Import Word files as notes** ticked and
  [pandoc](https://pandoc.org) installed, a dropped or picked `.docx` file is a "Word document" in the Import
  dialog and becomes a note: headings, emphasis, underline, sub- and superscript, links, lists, tables
  (with merged cells too), footnotes, math, highlights and pictures with their captions. Pictures and the
  kept original (box "Keep the original .docx") go into one folder `z_media/<year>/<month>/import_docx_<name>/`.
  Pandoc runs in the background with a two-minute limit; a file it cannot read leaves nothing in the vault
  and the message names the file and pandoc's first error line. The settings `docx_import` and
  `pandoc_path` are new; Settings has an Import tab that shows where pandoc was found, its version, and how
  to install it. Off, or without pandoc, a `.docx` is an attachment as before (IMP-035 to IMP-039, #295).
- **Portable file names.** `fsutil::portable_name` makes names that work on Linux, macOS and Windows (no
  characters Windows forbids, no trailing dot or space, no device names like `CON`, at most 127 bytes);
  the Word import uses it first (VAULT-026, #298).
- **Footnotes, underline, subscript, superscript and line breaks in table cells are shown** (#296).
  A footnote reference `[^1]` that has a definition `[^1]: text` (continuation lines indented by four
  spaces or a tab) is a small raised label in the link colour, with its brackets hidden by the Markdown
  preview; the definition shows its label raised and its text a little smaller, and a reference without a
  definition stays as written. In the reading view a click on a reference scrolls to its definition and a
  click on the label of a definition scrolls back. `<u>text</u>` is underlined, `<sub>…</sub>` lowered and
  `<sup>…</sup>` raised and smaller, read with the same rules as `<mark>` and `<span>` (any letter case,
  nesting, pairs only, nothing in code), and `<br>`, `<br/>` and `<br />` inside a cell of a table start a
  new line in the cell (the cell editor keeps the tag). All of it shows in the editor's Markdown preview,
  the reading view, table cells, the outline and the PDF export, where a footnote reference links to its
  definition. These are the forms pandoc writes for a Word document, and common in hand-written notes
  (EDIT-045, EDIT-050, EDIT-051, PDF-011).

- **Footnotes, underline, sub- and superscript and line breaks in table cells are written by the LaTeX export** (#296): `\footnote{…}` with the text of the definition, `\underline`, `\textsubscript`, `\textsuperscript`, and `<br>` as a line break in the cell (TEX-029).
- **Math is typeset** (#297). A formula between dollar signs, `$E = mc^2$`, is set in the text; `$$…$$` on a
  line of its own, `$$` lines around several lines, or a ```` ```math ```` block is set centred in display
  style. Shale does it itself, in pure Rust, with the bundled font STIX Two Math (SIL Open Font License) and
  its OpenType MATH table: no TeX is needed. Supported: letters in math italic, digits and operators
  upright, groups, super- and subscripts, `\frac` `\dfrac` `\tfrac` `\binom`, `\sqrt` and `\sqrt[n]`, Greek
  letters, relations, binary operators, arrows and symbols, big operators with limits (`\sum` `\prod` `\int`
  `\iint` `\oint` `\lim` …, `\limits`, `\nolimits`), function names and `\operatorname`, `\left…\right`
  delimiters that grow, accents, `\text`, `\mathrm` `\mathbf` `\mathit` `\mathbb` `\mathcal`, spaces, and the
  environments `matrix` `pmatrix` `bmatrix` `vmatrix` `cases` `aligned`. `$5 and $10` is no formula, and
  nothing in code or front matter is one. It shows in the reading view, the editor's Markdown preview (the
  cursor line shows its TeX), table cells, the outline and the PDF export (as sharp vector outlines). A
  formula Shale cannot typeset (an unknown command, unbalanced braces, more than 64 levels or 10 000
  characters) stays as its TeX in the code style, with the reason when you point at it. This is how a Word
  document's formulas, which the pandoc import writes as `$…$`, look (EDIT-046 to EDIT-049, PDF-012).

- **Math is written by the LaTeX export** (#297): a formula Shale can typeset goes to LuaLaTeX unchanged as `\(…\)` or `\[…\]`, one it cannot is written as code, so a note cannot run TeX through a formula; `%`, `#`, `&`, `^`, `_`, `$`, `~` inside `\text{…}` must now be written with a backslash (TEX-029).
- **Every name Shale writes is portable.** A name you type for a new note, folder or drawing, or for a
  rename (the title line too), is no longer taken as typed when Windows cannot have it: `< > : " / \ | ? *`
  and control characters become `-`, a trailing dot is dropped, a device name like `CON` gets `_`, and the
  name is cut to 200 bytes. The message tells it ("Saved as `a-b.md`: `:` is not allowed on Windows") and
  links, also a red link you followed, use the name the file got. The media folder, the imports and the
  people notes use names of at most 127 bytes. Existing files are not renamed by this (VAULT-027, #298).
- **Check names for Windows and macOS.** Settings → Vault lists, from a background job, the files and
  folders whose names would break on another system (forbidden characters, a trailing dot or space,
  device names, names over 200 bytes, paths over 240 characters, names that differ only in letter case),
  each with the name it would get. Rename one, or all after a question: links are rewritten as for any
  rename and a name that is taken gets ` 2`, ` 3` (VAULT-028, #298).
- **Esc keeps the cursor in place.** In Vim-style editing, Esc from insert mode now leaves the cursor on the
  character right after where you stopped typing instead of stepping back one character. Settings → Vim →
  "Esc from insert mode" (`esc_cursor` in the settings file) brings back Vim's step back (#303).
- **A Vim tab in Settings**, listed under Editor and drawn indented: the Vim-style editing switch, the cursor
  of normal mode, "Esc from insert mode" and the link preview size moved there from the Editor tab. Editor has
  a "Cursor" section for the cursor while typing and the blinking (#303).
- **A folder as one PDF, a book** (#306). **Export to PDF…** in the menu of a folder in the file list, and in
  the menu of the vault (right-click the empty space under the tree), writes the notes of the folder
  and its subfolders as one PDF: a title page, one table of contents, every note on a new page, numbered
  headings that continue the chapter's number, a small running header with the chapter's title, links
  between the chapters and nested bookmarks. A folder prints as a book without renaming anything: its
  notes are the chapters (`export: false` leaves one out), a folder note (`index.md`, `README.md`, the note
  named like the folder, or `folder-note: true`) introduces a folder and gives the book its `title`,
  `subtitle`, `author` and `date`, and the reading order is the folder note's `order:` list, else the order
  of its links, then the other notes and folders by name with numbers by value. The dialog has a folded
  **Reading order** to check the tree and runs the export in the background with Cancel.
  `shale --export-pdf <folder> <file.pdf>` writes the book without opening a window, and
  `scripts/userguide-pdf.sh` makes the PDF of the user guide, which now follows the convention
  (BOOK-001 to BOOK-012).
- **The user guide as a PDF**: every release has `shale-userguide-<version>.pdf` next to the
  guide's zip, the whole guide as one book to print or read anywhere, with its `.sha256` file and
  the build attestation (#310).

### Changed

- **Settings → Keybindings asks before it overwrites a key** (#294). A key that another action
  already uses is no longer refused: on Enter a question says “Ctrl+H is used by “…”. Overwrite it?”
  (naming every action it clashes with, for a sequence that is the start of others too), with **Yes**
  and **No**. No has the keyboard, so Enter is No; Esc is No, Y and N answer directly. Yes takes the key
  from the other action (it stays without a key, "not set") and saves the new one; No changes nothing.
  While you record, a note under the keys says which action they would clash with as soon as all keys
  are released. Keys that Shale must keep (plain letters for a shortcut that works everywhere, Ctrl+C
  and the like, Enter and Esc of the lists) are still refused with their message (KEY-018).
- **LaTeX export: the gaps of the writer** (#305). Fenced code sits on a light grey box and keeps its
  syntax colours; in documents a long block goes on on the next page. A picture inside a line of text
  (`an icon ![[icon.png]] here`, also in a table cell) is drawn in the line, as high as the line; a missing
  one says `[missing: name]` in red. A link to a heading of the same note (`[[#Results]]`,
  `[[Note#Results]]`) is clickable in the PDF and jumps to the heading; links to other notes are their text
  in the colour of links (red for a note that does not exist), as in the built-in export. A
  `latex-template:` that names a set or kind the vault does not have is no longer replaced silently: the
  export dialog says what was not found and what it uses instead (TEX-030 to TEX-033).
- The user guide shows the export dialog with a TeX that was found and the setup's test page when it
  worked; the screenshots use a pretend TeX made for them (TEX-034).
- The PDF export (of a note and of a book) reflows paragraphs: lines of text that are one paragraph in
  Markdown, such as a note wrapped at 100 columns, are joined and laid out to the page's width; a blank line,
  a heading, a list, a table, a fence, an embed or a hard break (two spaces, a backslash) still ends a line
  (PDF-009). Arrows (`→ ← ↑ ↓ ↗`), `⋯`, `▸` and `📅` are printed too, from the bundled Hack and emoji fonts
  (PDF-010).

### Fixed

- **Videos play with an older ffmpeg** (4.x, as on Ubuntu 22.04). Its `ffprobe` refused one of
  the questions Shale asks about a video, so every video was called damaged and showed no
  player and no first picture, also in the PDF export. Shale now asks again in the older form
  (#299).
- **A video plays on a computer without a sound output** (#304). Before, the player only said "No
  sound output" and showed nothing moving. Now the video plays its pictures without sound, with the
  note "No sound output: playing without sound". A sound file still says "No sound output".

### Internal

- The video tests no longer fail on slow machines (macOS CI): their pretend sound card keeps real
  time (#266).
- The tests of the Alt keys of the note buttons build the Linux/Windows keymap explicitly and check the macOS keymap, which has no Alt defaults, separately, so they pass on macOS too (KEY-002, KEY-014, KEY-015, #300).
- The TeX compile test compares the resolved work folder, so it passes on macOS where the temp folder is
  behind `/var` -> `/private/var`; it now runs its work folder through a symlink on Linux too (#301).
- **macOS: the closed-port test helper holds its port again.** The helper for tests of an
  unreachable server now holds a bound socket that does not listen, since macOS gave the port
  of a connected socket to a later bind with SO_REUSEADDR (#302).
- Spec row IDs are unique again: the picture and video resizing row is now EDIT-052 and the
  captions row EDIT-053 (they shared EDIT-024 and EDIT-025 with the table rows), and a test fails
  when a spec row ID is used twice (DEV-014, #307).
- **Two TeX export tests pass on Windows.** The search path test expected a Unix folder, and the
  install question test a Unix install folder; both now use what the product does on each
  system. `TEXINPUTS` is joined with kpathsea's separator (`;` on Windows, `:` elsewhere) by
  Shale itself, and a template folder whose name has the separator is refused instead of being
  quoted in a form TeX cannot read (#311).
- **One way to start external programs**: gpg, LuaLaTeX and ffmpeg/ffprobe are all started
  through `program::spawn`, which tries a program again that the system briefly calls "busy" right
  after it was written (a test stand-in under load). The ffprobe test no longer fails now and then
  with "Text file busy" (#316).
- **A flaky TeX setup test is steady again.** The TeX setup page tests now wait until no look for
  TeX is running and the page has settled before they check it, so the missing-TeX test no longer
  fails under load (#308).
- **Tests never open real windows.** A test build no longer starts the real `xdg-open` or shell
  call of the opener, so `cargo test` stops popping up PDF viewers (#313).

## [0.4.0] - 2026-11-17

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
- **Numbered headings.** The headings of a note (`#` and deeper) show a number before their text, `1`,
  `1.1`, `1.2`, `2`, drawn muted in the editor, the Markdown preview, the reading view, the outline, the
  link preview and the PDF; the number is never written into the file (the note's title is the title
  line above the text, not a `#` heading, #287). Settings → Editor → "Number headings" (on by default) turns it on or off, and a
  note's front matter `numbered-headings: true` or `false` overrides it for that note (LINK-014, CFG-042, #283).
- **Table of contents and bookmarks in the PDF.** The note export has a "Table of contents" checkbox (on):
  after the title, a *Contents* page lists the `#`–`###` headings with their numbers and the pages they
  start on, each entry a link; the text starts on the next page. The PDF also has bookmarks for the
  `#`–`######` headings, nested by level (PDF-008, #283).
- **Heading completion.** After `[[Note#` or `[[#` the link suggestions list the headings of that note, with
  their numbers, and insert the heading's plain text (LINK-016, #283).
- **Settings → LaTeX.** The first part of the PDF export through LaTeX templates: the page finds a
  LuaLaTeX (the one on your computer when its LaTeX kernel is from 2022-06-01 or newer and `biber` is
  next to it, else a TeX Live that Shale installs itself) and says which one is used or why none can
  be. **Install TeX Live…** shows the sizes (about 100 MB to download, 330 MB on disk) first and
  downloads nothing until you agree; it shows the progress, can be cancelled (nothing half-installed is
  left) and runs without touching anything outside Shale's data folder. **Update** and **Remove…**
  (which names the folder and its size) keep it tidy, **Test** compiles a small document with the fonts
  and a bibliography, and the new settings `tex.engine` and `tex.repository` choose the TeX and the
  download address. Compiling runs LuaLaTeX and biber as often as the log asks, in a temporary folder,
  without a shell, with a time limit, and turns the log into readable errors ("package x is missing",
  `file:line: message`). The export dialog that uses it follows with the next steps (TEX-001 to TEX-004,
  TEX-023, CFG-044, #279, #277).
- **Title line without the date prefix.** A note's title line (above line 1) and its tab show the file name
  without `.md` and without a leading `YYMMDD_` date prefix (`261117_Plan.md` shows `Plan`); renaming from
  the title line keeps the prefix. The file list, the quick switcher and links keep the full name
  (VAULT-025, #287).
- **Date prefix for new notes.** Settings → Editor → "Date prefix for new notes" (on by default): a note
  made by Shale (new note, red link, quick switcher, new drawing) is named `YYMMDD_<name>.md` with today's
  date. A red link `[[Plan]]` that makes `261117_Plan.md` is rewritten in its note to `[[261117_Plan|Plan]]`
  so it still reaches the note; imports and daily notes keep their names (CFG-043, #287).
- **Markdown files become notes.** A dropped or picked `.md` / `.markdown` file is imported as a note with
  its text unchanged (listed as "Markdown note") instead of an attachment of a Files note (IMP-033, #288).
- **Lift headings.** When an imported Markdown note has headings but no `#`, the Import dialog has a ticked
  box "Lift headings (## → #, ### → ##, …)": every heading moves up by the same number of levels so that the
  highest becomes `#`; code blocks and front matter stay as they are (IMP-034, #288).
- **Lift headings on paste.** Pasting text whose headings are all `##` or deeper asks "The pasted text has no
  # heading. Lift its headings?" with Lift and Paste as it is (Esc pastes as it is) (EDIT-038, #288).
- **Edit accounts without removing them** (#290). Settings → Calendars: **Edit** on a CalDAV
  calendar changes its address, user name or password (empty password = keep the saved one); a new
  user name or password also goes to the other calendars of the same server and user name. Feeds →
  Accounts…: **Edit** signs in to an account again (a new app password, code or token) and replaces
  its secret, keeping its feeds, posts and open tabs.
- **Alt keys for the buttons of a note** (#292). Every button of the formatting toolbar, the view
  header (edit / reading view, Markdown preview, the ⋯ menu) and the tab bar (New tab) has an Alt key
  (Alt+B bold, Alt+I italic, Alt+E heading menu, Alt+V reading view, Alt+P preview, Alt+N new tab, …),
  and the ribbon's first ten icons answer to Alt+1 … Alt+9 and Alt+0, the sidebars to Alt+, and Alt+.
  The keys work with the keyboard in the text and type nothing. Hold Alt and a small badge with the key
  shows at each button (it follows a key you changed). They are definable in Settings → Keybindings (new
  scope Note view); on macOS they have no default key. A test now checks that no two keys that can be
  active together clash, on every system.
- **Cursor shapes.** Settings → Editor → "Cursor while typing": a bar before the character (as
  before), a bar after it, a block or an underline; "Cursor in normal mode" (Vim-style editing)
  chooses the cursor of normal mode, a block by default; "Blinking cursor" (on by default) can be
  turned off. Exactly one cursor is drawn in every mode: in normal mode the block no longer comes
  with a bar of the text box. The settings file has `cursor_shape`, `normal_cursor_shape` and
  `cursor_blink` (EDIT-039, EDIT-040, EDIT-041, #293).

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
- **`#` is numbered.** With numbering on, a `#` heading is number 1 like any other level (`#` 1, `##` 1.1,
  `###` 1.1.1); the title of a note is its title line (LINK-014, #287).
- **PDF title once, contents `#`–`###`.** The contents page lists the `#`–`###` headings and the bookmarks
  cover `#`–`######`; the title is printed from the title line (without the date prefix), and not a second
  time when the note's first heading is a `#` that says the same (PDF-008, #287).
- The user guide, the sample vault and the screenshot fixtures lost the `# Name` line that repeated the
  title line of each note (#287).
- After a click on a toolbar button, the edit / reading, preview or sidebar buttons or an icon that only
  opens a sidebar panel, the editor has the keyboard again with the cursor and selection where they were.

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
- **The keyboard is no longer lost when a pane closes** (#291). Closing the last tab of a split
  pane (or any tab, or the pane itself) now puts the keyboard into the editor of the open note of
  the focused pane. As a guard, when the widget that had the keyboard disappears and nothing else
  takes it, the editor gets it back (not while the sidebar is browsed, a dialog is open or the
  mail view is shown).

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

[Unreleased]: https://github.com/rabalyn/shale/compare/v0.7.6...HEAD
[0.7.6]: https://github.com/rabalyn/shale/compare/v0.7.5...v0.7.6
[0.7.5]: https://github.com/rabalyn/shale/compare/v0.7.4...v0.7.5
[0.7.4]: https://github.com/rabalyn/shale/compare/v0.7.3...v0.7.4
[0.7.3]: https://github.com/rabalyn/shale/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/rabalyn/shale/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/rabalyn/shale/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/rabalyn/shale/compare/v0.6.10...v0.7.0
[0.6.10]: https://github.com/rabalyn/shale/compare/v0.6.9...v0.6.10
[0.6.9]: https://github.com/rabalyn/shale/compare/v0.6.8...v0.6.9
[0.6.8]: https://github.com/rabalyn/shale/compare/v0.6.7...v0.6.8
[0.6.7]: https://github.com/rabalyn/shale/compare/v0.6.6...v0.6.7
[0.6.6]: https://github.com/rabalyn/shale/compare/v0.6.5...v0.6.6
[0.6.5]: https://github.com/rabalyn/shale/compare/v0.6.4...v0.6.5
[0.6.4]: https://github.com/rabalyn/shale/compare/v0.6.3...v0.6.4
[0.6.3]: https://github.com/rabalyn/shale/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/rabalyn/shale/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/rabalyn/shale/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/rabalyn/shale/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/rabalyn/shale/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/rabalyn/shale/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/rabalyn/shale/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rabalyn/shale/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rabalyn/shale/compare/6851a04...v0.2.0
[0.1.0]: https://github.com/rabalyn/shale/tree/6851a043e3c1f98af9b92575a43a93202b3b13b7

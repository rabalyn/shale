# Shale features

Shale is a notes app for the Linux desktop. Your vault is a plain folder of markdown files,
and notes link to each other with `[[wiki links]]`. The screenshots below come from the app's
built-in demo vault (`shale --screenshot out.png`).

## Notes and links

![The editor](screenshots/editor.png)

- `[[Note]]` links are **blue** when the note exists and **red** when it doesn't. Click a blue
  link to open the note, or a red one to create it.
- Pictures (`![[photo.png]]`), drawings (`![[Sketch.excalidraw]]`) and PDFs are shown inside the
  editor.
- Lists continue when you press Enter (also numbered lists and to-dos `- [ ]` / `1. [ ]`).
  Tab and Shift+Tab indent list items or change heading levels, and Alt+↑/↓ move lines.
- Web links (`[text](https://…)`) open in your browser.
- Drag notes, drawings, files and folders in the file list to move them into another folder.
  Links that point to them by path are updated.

![Moving a note into a folder](screenshots/tree-drag.png)

- Back and forward with the mouse side buttons or Alt+←/→. You can sort the file list by name,
  date modified or date created.

![Suggestions while typing](screenshots/link-suggestions.png)

- Type `[[` to get matching notes; the last entry creates a new note. Esc closes the list.
- Type `[!` to pick a file: `/` starts at the vault, `~/` at your home folder, and Tab completes
  like a shell. Files from outside the vault are copied to `z_media/<year>/<month>/`.

![Reading view](screenshots/reading.png)

- Ctrl+E switches between editing and the reading view.

![The note menu](screenshots/more-menu.png)

- Export notes and drawings to PDF (paper size, margins, fonts and more in a dialog), insert a
  new drawing, rename the note (links in other notes are updated), or move it to the trash.

## Keyboard only: sidebar, file list and vim-style editing

- Ctrl+H puts the keyboard into the left sidebar, Ctrl+L back into the note. Ctrl+Shift+B opens
  the file list with the open note selected: j / k go from file to file, Ctrl+J / Ctrl+K from
  folder to folder, Space opens or closes a folder, Enter opens a file.
- In the search results j / k go from hit to hit and Ctrl+J / Ctrl+K from file to file.
- **Vim-style editing** (Settings, off by default): notes open in normal mode. h j k l, w b,
  W B, 0 $, gg G move; `:12` jumps to line 12; `/text` searches and n / N go on; i a I A o O
  start typing, Esc stops; x deletes a character; v and V select with the same motions, d cuts
  and y copies to the clipboard, p pastes. The status bar shows NORMAL, INSERT, VISUAL or V-LINE.

## Drawings

![A drawing with the style panel](screenshots/drawing.png)

- Hand-drawn style sketches, saved as standard `.excalidraw` files.
- Drawings of the Obsidian Excalidraw plugin (`.excalidraw.md`, compressed or not) open in the
  drawing editor too, and are saved in the plugin's own format, so Obsidian keeps reading them.
  Links like `[[Drawing.excalidraw]]` and embeds `![[Drawing.excalidraw]]` find them.
- Links inside drawings: type `[[` in a label or text for note suggestions, `[[p ` for people
  and `[!` for the file picker, just like in notes. A shape or text with a link gets a small ↗
  mark; click it (or Ctrl+click the shape) to open the note, Shift for a new tab.
- Links in drawings and canvases count like links in notes: the linked note lists the drawing
  under Backlinks, and the graph shows the connection. Renaming or moving a note updates the
  links in drawings too; canvases are never changed, so the message names any canvas that still
  points to the old name.
- Pictures in drawings show up: pictures of Obsidian Excalidraw drawings (from the vault) and of
  excalidraw.com files (stored in the file), also in drawings embedded in notes and in PDF
  export.
- Tools on keys 1–0: hand, select, rectangle, diamond, ellipse, arrow, line, pen, text, eraser.
- The style panel sets stroke and fill colours, fill style, stroke width, outline pattern,
  sloppiness, corners, label font, size, colour, background and alignment, opacity, and the
  order of shapes.
- Double-click a shape to label it. Arrows snap to shapes and follow them when shapes move.
  Hold Shift to keep lines at 45° steps.
- "Auto arrange" untangles a diagram; "Spread out" and "Pull together" change the spacing.

### Obsidian canvases

![A canvas shown read-only](screenshots/canvas.png)

- Obsidian Canvas files (`.canvas`) open in the drawing view: cards with their colours, groups,
  file and web cards (click them to follow), and the arrows between cards with their labels.
- Opening one asks whether to convert it. **Convert** makes an editable drawing
  `Name.excalidraw.md` next to it (Obsidian opens it with the Excalidraw plugin); **Open
  read-only** just shows it. The canvas file itself is never changed. Opening the canvas again
  offers the drawing made before.
- Tick "Always convert canvases when opening them" (also in Settings → Drawings) to skip the
  question.

![The question when a canvas is opened](screenshots/canvas-convert.png)

## To-do list across all notes

![Tasks](screenshots/tasks.png)

- Every `- [ ] task` from every note in one list. Ticking a task off saves it in its note.
- Filter by text or `#tag`, show done tasks, or only tasks with a due date
  (`📅 2026-12-01` or `due:2026-12-01`).

## Linked notes and the graph

![Graph](screenshots/graph.png)

- The right sidebar shows backlinks, unlinked mentions, outgoing links, the outline and a local
  graph of the open note.
- The vault graph shows every note. Red dots are missing notes and green ones are drawings.

## Calendar

![Month view](screenshots/calendar-month.png)

- A calendar in the right sidebar (small month) and as a big tab, with Month, Week and Day views
  in three sizes.
- Local calendars (`.ics` files, for testing; with sample events) and **CalDAV** calendars
  (for example Nextcloud). Each CalDAV calendar has its own automatic sync interval (15 minutes
  by default).

![Week view and event details](screenshots/calendar-week.png)

- Click an event to see its details: when, how it repeats, where, organizer, attendees and
  notes. **Edit** and **Delete** work for local and CalDAV calendars.

![New event](screenshots/event-dialog.png)

- Double-click a day or time to create an event. It starts at the next full or half hour and is
  30 minutes long.
- "Save to" chooses any calendar. When a calendar refuses the save (for example because you have
  no write permission), the error is shown in the dialog and you can pick another calendar.
  Moving an event writes it to the new calendar first and removes it from the old one only after
  that worked, so nothing is lost.

![Calendar settings](screenshots/calendar-settings.png)

- Settings → Calendars: add local calendars or CalDAV accounts ("Find calendars" asks the
  server), choose colours, show or hide calendars, and set the sync interval per calendar.

![Calendar in the sidebar](screenshots/calendar-sidebar.png)

## Importing e-mails, events and files (for project tracking)

![Import dialog](screenshots/import-dialog.png)

- Drag e-mails from Thunderbird, events from a calendar (Shale's own or another program), `.eml`
  or `.ics` files, or any other files (plans, PDFs…) onto the window. The ribbon's Import button
  opens the same dialog.
- Choose the folder (one per project, for example) and tags. Small pictures from signatures are
  left out by default, and each attachment can be ticked on or off.
- Drop onto a folder in the file list to import into that folder, or onto a note to also add
  links to the new notes there.
- Messages that were already imported are recognised and left out unless you tick them again.
- On Wayland, Shale reads what Thunderbird hands over. When that is only a link to the message,
  Shale finds the message in Thunderbird's local copy (for IMAP accounts this needs "Keep
  messages for this account on this computer").

![Dragging an event onto a folder](screenshots/drag-event.png)

![An imported e-mail](screenshots/imported-note.png)

- Each e-mail becomes a note named "date subject" with the sender, recipients and date, the tags
  in its properties, links to the attachments and the original message, and the text of the
  message. The tag list counts these tags too.

## Feeds: read Mastodon and Bluesky, save posts to the vault

![The feed view](screenshots/social-feed.png)

- The ribbon's speech-bubble button opens **Feeds**. **Accounts…** adds a Mastodon account (type
  the server, open the sign-in page in the browser, paste the code it shows; or paste an access
  token) or a Bluesky account (handle and an **app password** from Settings → Privacy and
  security → App passwords, never your main password). Secrets go to the system keyring.
- Pick the account and the feed (Mastodon: Home and your lists; Bluesky: Following and your
  saved feeds). Posts show the avatar, name, time, "boosted by", pictures (video and GIF
  previews have a play mark), quoted posts and link cards. A content warning stays closed until
  you click Show. The next page loads when you reach the end of the list; Refresh starts over.
- **Save to vault** copies the post, and the whole thread when it is a thread by one author,
  into the Import dialog: choose the folder (default `Social`), tags and which pictures to
  bring along. **Open in browser** and **Copy link** are next to it.

![Saving two posts](screenshots/social-import.png)

- Tick **Include replies from others (N)** to add a "Replies" section with other people's
  answers, nested like a conversation (off by default; Shale remembers your choice). Their
  pictures are linked, not downloaded.
- The note has the author, the date, a **Source** link back to the post, the text (a thread:
  every post in order, numbered), the pictures saved in `z_media/` and their descriptions. A post
  that was saved before, also as part of a thread, is marked "Imported before".
- Try it without an account: `shale --screenshot out.png --view social` shows made-up posts.

## People (contact notes)

![People list](screenshots/people-picker.png)

- Contact notes live in the **people folder** (default `Adressbuch`, changeable in Settings).
- Type `[[p ` and a name to pick a person from that folder. The link that is written is a normal
  `[[Name]]` link, so other markdown tools understand it too. "New contact" makes a contact note
  from a template (e-mail, phone, organization, notes).
- When importing, the sender, recipients, organizer and attendees are found and matched to
  contact notes (by e-mail address anywhere in the note, else by name; "Müller, Hans" = "Hans
  Müller"). Known contacts and new senders are ticked; for ticked people without a note one is
  made. The imported note gets a "People" line with links, so each contact note's backlinks show
  every mail and meeting with that person. Automatic senders like "noreply" are left out.

![Import with people and tags](screenshots/people-import.png)

## Tags found automatically (no server, no AI)

- Tags of the vault that are written in the imported text are set right away
  (`#lecture-hall-b` for "Lecture Hall B", the last part of nested tags too).
- Shale also learns, from the notes in the vault, which words are typical for each tag (words in
  most notes with the tag and rarely elsewhere) and suggests those tags when an import contains
  several of these words. It is plain counting: the same notes always give the same suggestions.
  It is learned again in the background now and then and kept in `.shale/tag-words.json` in the
  vault.

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Edit / reading view |
| Ctrl+, | Settings |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Keyboard into the left sidebar / back into the editor |
| Ctrl+Shift+B | File list with the keyboard in it: j / k next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results: j / k next / previous hit, Ctrl+J / Ctrl+K next / previous file, Enter opens the note at the hit, Esc back to the box |
| Alt+← / Alt+→ | Back / forward |
| Alt+↑ / Alt+↓ | Move the current line |
| Tab / Shift+Tab | Indent lists or change heading levels |

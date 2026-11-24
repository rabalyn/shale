# Spec: busy states, the inventory of long actions (JOB)

**IS NOW** = true of the code today · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Issue: #397.

The rules are JOB-003 to JOB-039 in [engineering.md](engineering.md#background-jobs-job): every action that
loads or runs longer than a moment is a task of the busy registry (`busy::Busy`), shows a spinner and
what happens, cannot be started twice, offers Cancel where the work can stop (or Close for a dialog
whose work goes on), and shows in the indicator at the top right of the editor pane while it runs.
This page lists every such action as of 2026-11-22: where it runs today, whether the work can stop,
what the window shows today, and where it stands on the registry. Rows marked *planned* are the
work of the follow-up issues of #397 (JOB-009); JOB-010 says which dialogs show Close and which Cancel.

"Thread" means `jobs::spawn` / `jobs::run` (JOB-001, JOB-002): a worker thread whose result always
arrives. "UI thread" means the window itself waits, the worst case this page is about. "Stop" is
whether a flag or a dropped job ends the work early today; *unsafe* means that stopping it half way
would leave a mess (JOB-006).

## Notes, vault, files

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Open a vault (`ShaleApp::open_vault_in_background`, `Vault::open`: read and index every note, NET-019) | thread (`app/vault_load.rs`); headless paths (screenshots, tests) read on the calling thread | yes: the result is ignored when it ends | the start screen: spinner, "Opening ‹folder›…" and Cancel | **IS NOW** (NET-019): task `vault-open` |
| A close or switch of the vault that waits for a running sync (VAULT-036, VAULT-037) | none: a pending action that `vault_leave_tick` carries on each frame once the sync has ended (30 s at most, counted over frames) | yes: Cancel drops it and keeps the vault open | "Waiting for the sync to finish…" with Cancel in the indicator | **IS NOW**: task `vault-leave-wait` |
| Rescan of the vault folder (every 3 s or longer, NET-013) | thread (`app/rescan.rs`) | no (the next scan is skipped while one runs) | nothing | **IS NOW** (JOB-022): task `vault-rescan`, "Reading the vault folder…", listed only when a scan takes longer than 2 s (a slow network share); no Cancel |
| Vault check (names and outside links, VAULT-028, MEDIA-022) | thread | yes, between two folders | spinner and "Checking file names and links…" with Cancel, button off | **IS NOW** (JOB-012) |
| Search in all notes (sidebar and picker, NAV-024, QUERY-014) | thread (`search_all/worker.rs`, one per window); a numbered query per queue, a newer one gives up the older (QUERY-017) | yes: Cancel in the indicator gives the running query up (the same flag the next keystroke sets); the last list stays | the last list stays, with a small spinner next to the source buttons (the list's own, QUERY-018) | **IS NOW** (JOB-038): task `search` per queue (picker, sidebar), "Searching…", listed only when a query runs longer than 2 s, with Cancel; the lanes of a board have none |
| Reading dropped files, looking a mail up in Thunderbird's folders (`app/drops.rs`) | thread | no | the hint at the bottom of the window | **IS NOW** (JOB-023): one task per read, "Reading the dropped files…"; no Cancel |
| Copy the outside media of a note into the vault (MEDIA-021) | thread (`app/outside_copy.rs`) | yes, between two files (the copied ones keep their new links) | a toast when done | **IS NOW** (JOB-023): task `outside-copy`, "Copying media into the vault…", with Cancel |
| Open an outside media file with the system's program (MEDIA-017) | thread (`app/outside_open.rs`) | no | a busy card in the player | **IS NOW** (JOB-023): one task per file, "Opening ‹name›…"; no Cancel |
| Tag learning (`import/tagwords.rs`) | thread | yes, between two notes (nothing is written) | nothing | **IS NOW** (JOB-022, JOB-023): task `tag-learning`, "Learning the words of the tags…", listed after 2 s, with Cancel |
| Reading a bibliography (`.bib` file, `bibliography/state.rs`, EDIT-123) | thread | no (one file is read and nothing written) | the status in Settings → Vault, "Reading the bibliography…" | **IS NOW** (JOB-039): task `bibliography-read` per file, "Reading the bibliography “refs.bib”…", listed after 2 s; no Cancel |
| Data usage: counting, Clear, Prune (USE) | thread (`data_usage`) | no | the task's spinner and text at the cache, "counting…" in the dialog | **IS NOW** (JOB-023): one task per cache ("Clearing the social pictures…", "Cleaning up …"); the two counts and a prune are listed after 2 s; no Cancel |
| Link preview download (`link_preview.rs`) | thread, one at a time | no (a timeout ends it) | spinner in the preview card | not a task: it belongs to its card |
| PDF page rendering of the reader (`pdf/view.rs`) | thread | no | spinner in the page | not a task: it belongs to its page |
| Picture and video loading (`media/widget.rs`) | thread / player | no | spinner in the widget | not a task: it belongs to its widget |

## Git sync, history, LFS

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Git sync (`gitsync::sync`, every interval and "Sync now", SYNC-002) | thread | unsafe (between commit and push) | spinner and the text, "Sync now" off | **IS NOW** (JOB-011): "Syncing with git…" |
| Store media with Git LFS (SYNC-017) | thread | unsafe | spinner and the text, button off | **IS NOW** (JOB-011): "Setting up Git LFS…" |
| LFS download of a file or of the vault (SYNC-020) | thread, one at a time, a queue behind it | no (`git lfs pull` writes the files itself) | the player's busy card; spinner and text in Settings → Sync | **IS NOW** (JOB-026): "Downloading ‹file› from Git LFS…" |
| File history: snapshot, thinning and queries (HIST) | thread (`app/history.rs`) | no (a snapshot waits for the tree lock) | the panel's "Reading the history…" with a spinner | **IS NOW** (JOB-025), listed in the indicator after 2 s (JOB-024) |
| Turn-on wizard: detect, test sync, provider calls, move out, Advanced connect (SYNC-047…) | thread (`gitsync/wizard`) | dialog calls: yes, at the next safe point (before the token is kept, the key or repository made); the move out and Advanced connect: no | spinner and text on the wizard page and in Settings → Sync, Next off | **IS NOW** (JOB-027): closing the dialog stops its call (JOB-010) |
| Managed git: detect, install, check for a newer one, remove (SYNC-023…) | thread (`gitsync/install_ui.rs`) | install, update and the check: yes (flag, what was downloaded is removed); removal: no | spinner, text, a progress bar and Cancel | **IS NOW** (JOB-028) |

## Export

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| PDF export of a note or drawing (built-in, PDF-001) | thread (`pdf/note_export.rs`) | yes, before each picture; nothing is written when it stops | spinner and "Exporting to PDF… Cancel stops it.", Export off | **IS NOW** (JOB-017): "Exporting “Notes” to PDF…" |
| PDF export of a folder, a book (BOOK-010) | thread | yes, between notes | spinner and "Laying out the book: 3 of 12 notes" with Cancel | **IS NOW** (JOB-013): "Exporting “Handbook” to PDF…" |
| PDF export through LaTeX (TEX-016) | thread (compile) | yes (flag ends the process) | spinner "Compiling with LuaLaTeX… Cancel stops it." | **IS NOW** (JOB-018): "Exporting “Notes” to PDF with LaTeX…"; the dialog keeps Cancel (JOB-010) |
| Export to Word (DOCX-002) | thread (pandoc) | yes (flag ends the process) | spinner and "Exporting to Word… Cancel stops it." | **IS NOW** (JOB-019): "Exporting “Notes” to Word…"; the dialog keeps Cancel (JOB-010) |
| `shale --export-pdf` (BOOK-011) | the main thread, no window | no | the terminal | out of scope (no window) |
| Publish the vault as a site (`site_export/publish.rs`, SITE-065) | thread (`site_export/job.rs`), git as child processes | yes: Cancel kills the running git; the remote's branch is changed only by the last push | spinner, "Publishing the site…", the step ("Writing the site…", "Uploading the media files…") and Cancel; the button is off | **IS NOW**: task `site-publish` |
| Look at who published the target last (SITE-072) | thread | yes | spinner and "Looking at the publish target…" in Settings → Publishing; listed in the indicator after 2 s | **IS NOW** (JOB-022): task `site-publish-look` |
| `shale export-site --publish` (SITE-066) | the main thread, no window | no | the terminal: the steps on stderr | out of scope (no window) |

## Import

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Import dialog: converting Word files with pandoc (IMP-035) | thread | yes (flag) | spinner and "Converting the Word files with pandoc… Cancel stops it.", Import off | **IS NOW** (JOB-020); the dialog keeps Cancel (JOB-010) |
| Import of mails, events, files into notes (`import_action`) | UI thread | no | nothing (usually quick) | not moved: it needs the vault and the sync tree held, and writes one history snapshot (IMP-070…IMP-074, JOB-020); planned as a thread only when it proves slow |
| Mail import from the mail view (MAIL-070) | thread (`app/mail.rs`) | no | nothing until the dialog opens | **IS NOW** (JOB-023): "Reading the mails…" (the same read as a drop) |
| Import of saved web pages from the spool of the browser extension (CLIP-009, CLIP-067, #427) | none yet | no (a half-written clip is worse than a late one; the rest waits in the spool) | not built | planned with the clipper: a task "Importing saved web pages…" on a worker thread (SHOULD BE, CLIP-067) |
| Pandoc probe, TeX probe (settings) | thread | no | the task's spinner and text | **IS NOW** (JOB-023): tasks `pandoc-probe` ("Looking for pandoc…") and `tex-probe` ("Looking for TeX on this computer…"); no Cancel |

## TeX, updates

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| TeX install, update, removal (managed TeX Live, CTAN packages, TEX) | thread (`pdf/tex/install`) | install and update: yes (handle); removal: no | spinner, progress bar and Cancel in Settings and the setup dialog | **IS NOW** (JOB-029) |
| TeX test compile, test page of a set and template set import (`tex/settings_ui.rs`, `sets_ui.rs`) | thread | import: yes (handle); the two compiles: no (the compile does not look at the flag) | spinner and text, progress and Cancel for the import | **IS NOW** (JOB-029) |
| Update check, download, install (UPD, `update/mod.rs`) | thread, one `cancel` flag for all three | check and download: yes; install: no (a few file operations that undo themselves on an error) | spinner next to "Check for updates now"; the notice shows the text with a spinner and Cancel | **IS NOW** (JOB-030): "Checking for updates…", "Downloading Shale ‹version›…", "Installing Shale ‹version›…" |

## Mail

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Opening the mail store (`MailStore::open`, MAIL-050; the integrity check, MAIL-006, and a possible index rebuild) | thread, started with the first frame of the window when an account is set up (else when the mail view first opens) | no | a spinner and "Checking the mail index…" in the view ("Opening the local mail copy…" in the instant between) | **IS NOW** (JOB-015) |
| Rebuilding the mail index (USE-012) | job of the Data usage dialog, which asks the store's writer thread | yes: between two folders (each folder is rebuilt in one transaction, so none is left empty) | a spinner, "Rebuilding the mail index…" and Cancel in the mail view | **IS NOW** (JOB-016) |
| Mail sync (IMAP, MAIL) | thread per account (`imap_sync::start`) | yes: before connecting, between two folders and between two batches of messages (never inside a file or an index write, MAIL-006/007) | progress in the view, "Cancel sync"; each running account with a spinner, its text and Cancel | **IS NOW** (JOB-021): "Syncing ‹account›…", one task per account |
| Mail: loading a listing, reading and downloading a message, search, key and GnuPG work | threads of the mail UI (`mail/ui.rs`, `send.rs`) | no | per-view lines ("Loading the list from the server…", "Reading the message…", "Searching…") | not tasks (they belong to their view); send and sign-in are tasks (next rows) |
| Sign-in (OAuth2) of a mail account (`start_sign_in` in `mail/ui.rs`, `oauth::sign_in`, MAIL-086) | thread per account | yes: while it waits for the browser (every 25 ms, then the redirect listener is closed), before the code is traded and before the tokens are kept; a timeout of 5 minutes | the account dialog's "Waiting for the browser…" with Cancel (and Copy the sign-in address); the account's folder list in the mail view with a spinner, its text and Cancel | **IS NOW** (JOB-032): "Signing in ‹account› in the browser…", one task per account; closing the dialog stops it |
| Sending mail (`send::send_due`, MAIL-132) | thread per account (`start_send` in `mail/ui.rs`) | yes, only before the next mail is taken from the Outbox: a mail whose data is on its way cannot be taken back (MAIL-133), so it is finished and the rest stays queued | the running account with a spinner, "Sending mail from ‹account›…" and Cancel in the mail view; a failure in the problem line and the Outbox | **IS NOW** (JOB-031): one task per account |
| Mail watch (IDLE, `mail/watch.rs`) | a thread of its own for the session | yes | nothing | not a task (JOB-021): it waits for the whole session; only a sync that runs is a task |

## Calendar and feeds

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| CalDAV load and sync of a calendar (CAL) | thread per calendar (`calendar/store.rs`) | yes: before the state request, between it and the events request, and the answer is dropped when Cancel came meanwhile; nothing is written by the load | the task with its text and Cancel under the calendar's status line in the settings; a small spinner in the calendar header | **IS NOW** (JOB-033): "Syncing the calendar “Work”…", one task per calendar; the timer's load is listed after 2 s, a load a person asked for at once |
| CalDAV account discovery (`calendar/dialog.rs`) | thread | yes: before each request; closing the dialog stops it | the task with its text and Cancel under the CalDAV fields | **IS NOW** (JOB-034): "Looking for the calendars of ‹server›…" |
| Social feeds: refresh, load more, check, fill a gap (SOC) | thread (`social/state.rs`) | yes: before the request, its answer is dropped when Cancel came meanwhile (the list stays as it was) | the task with its text and Cancel in the row of the check and at the end of the list; spinners inside the buttons | **IS NOW** (JOB-035): "Loading “Home” of @name…", "Loading more posts of “Home”…" (after 2 s), "Checking the feed of @name…" (the timer's after 2 s), "Loading the posts missing in “Home”…" |
| Social feeds: save a post (SOC) | thread (`social/state.rs`, `social/note.rs`) | yes: before the thread and before each file; nothing is kept | spinner inside "Save to vault" | **IS NOW** (JOB-036): "Saving the post of @name…" |
| Social accounts: opening the sign-in page, sign-in, new sign-in (`social/accounts_ui.rs`) | thread | yes: before each request and before the secret is stored; no partial token is kept; closing the dialog stops it | the task with its text and Cancel in the dialog's footer | **IS NOW** (JOB-037): "Opening the sign-in page of ‹server›…", "Signing in to ‹server›…" |
| Social: list of feeds, a conversation, marking notifications read, reactions, votes, sending, scheduled posts (SOC) | thread | no | spinner in the card, box or list that asked | not tasks: they belong to their card or box (SOC-045, SOC-050, SOC-055, SOC-062, SOC-064) |
| CalDAV: saving, moving or deleting an event (CAL-003) | thread (`calendar/store.rs`) | no | spinner in the event dialog | not a task: it belongs to the event dialog |
| Social images (`social/images.rs`) | worker threads | no | a placeholder | not a task: it belongs to its picture |

## How a planned row is done

Each follow-up issue moves its actions the way JOB-011 to JOB-013 did: a `Key` constant and a text, the
start through `Busy::spawn` / `spawn_cancellable` (or `Busy::begin` for work that has a thread of its
own), the buttons through `busy::ui::button`, the spinner through `busy::ui::status`, Cancel where the
"Stop" column says yes, and a test per action: the button is off while it runs, Cancel stops it, the
indicator lists it and drops it.

# Spec: busy states, the inventory of long actions (JOB)

**IS NOW** = true of the code today · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Issue: #397.

The rules are JOB-003 to JOB-015 and JOB-030 to JOB-036 in [engineering.md](engineering.md#background-jobs-job): every action that
loads or runs longer than a moment is a task of the busy registry (`busy::Busy`), shows a spinner and
what happens, cannot be started twice, offers Cancel where the work can stop (or Close for a dialog
whose work goes on), and shows in the indicator at the top right of the editor pane while it runs.
This page lists every such action as of 2026-11-22: where it runs today, whether the work can stop,
what the window shows today, and where it stands on the registry. Rows marked *planned* are the
work of the follow-up issues of #397 (JOB-009, JOB-010); #526 moved the git, TeX and update rows.

"Thread" means `jobs::spawn` / `jobs::run` (JOB-001, JOB-002): a worker thread whose result always
arrives. "UI thread" means the window itself waits, the worst case this page is about. "Stop" is
whether a flag or a dropped job ends the work early today; *unsafe* means that stopping it half way
would leave a mess (JOB-006).

## Notes, vault, files

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Open a vault (`ShaleApp::open_vault_in_background`, `Vault::open`: read and index every note, NET-019) | thread (`app/vault_load.rs`); headless paths (screenshots, tests) read on the calling thread | yes: the result is ignored when it ends | the start screen: spinner, "Opening ‹folder›…" and Cancel | **IS NOW** (NET-019): task `vault-open` |
| A close or switch of the vault that waits for a running sync (VAULT-036, VAULT-037) | none: a pending action that `vault_leave_tick` carries on each frame once the sync has ended (30 s at most, counted over frames) | yes: Cancel drops it and keeps the vault open | "Waiting for the sync to finish…" with Cancel in the indicator | **IS NOW**: task `vault-leave-wait` |
| Rescan of the vault folder (every 3 s or longer, NET-013) | thread (`app/rescan.rs`) | no (the next scan is skipped while one runs) | nothing | planned: listed only when a scan takes longer than 2 s ("Reading the vault folder…", a slow network share) |
| Vault check (names and outside links, VAULT-028, MEDIA-022) | thread | yes, between two folders | spinner and "Checking file names and links…" with Cancel, button off | **IS NOW** (JOB-012) |
| Search in all notes (sidebar, NAV-024) | UI thread, debounced; once per query and vault state (PERF-004) | no | nothing | planned if it is slow in a big vault: the list is made on a thread with "Searching…" |
| Reading dropped files, looking a mail up in Thunderbird's folders (`app/drops.rs`) | thread | no | the busy flag of the drop zone | planned: "Reading the dropped files…" |
| Copy the outside media of a note into the vault (MEDIA-021) | thread (`app/outside_copy.rs`) | no | a toast when done | planned: "Copying media into the vault…" |
| Open an outside media file with the system's program (MEDIA-017) | thread (`app/outside_open.rs`) | no | a busy card in the player | planned: "Opening …" |
| Tag learning (`import/tagwords.rs`) | thread | no | nothing | planned: listed only after 2 s |
| Data usage: counting, Clear, Prune (USE) | thread (`data_usage`) | no | "counting…" in the dialog, a spinner at the cache | planned: "Clearing the link previews…" |
| Link preview download (`link_preview.rs`) | thread, one at a time | no (a timeout ends it) | spinner in the preview card | not a task: it belongs to its card |
| PDF page rendering of the reader (`pdf/view.rs`) | thread | no | spinner in the page | not a task: it belongs to its page |
| Picture and video loading (`media/widget.rs`) | thread / player | no | spinner in the widget | not a task: it belongs to its widget |

## Git sync, history, LFS

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Git sync (`gitsync::sync`, every interval and "Sync now", SYNC-002) | thread | unsafe (between commit and push) | spinner and the text, "Sync now" off | **IS NOW** (JOB-011): "Syncing with git…" |
| Store media with Git LFS (SYNC-017) | thread | unsafe | spinner and the text, button off | **IS NOW** (JOB-011): "Setting up Git LFS…" |
| LFS download of a file or of the vault (SYNC-020) | thread, one at a time, a queue behind it | no (`git lfs pull` writes the files itself) | the player's busy card; spinner and text in Settings → Sync | **IS NOW** (JOB-032): "Downloading ‹file› from Git LFS…" |
| File history: snapshot, thinning and queries (HIST) | thread (`app/history.rs`) | no (a snapshot waits for the tree lock) | the panel's "Reading the history…" with a spinner | **IS NOW** (JOB-031): quiet tasks, listed in the indicator after 2 s (JOB-030) |
| Turn-on wizard: detect, test sync, provider calls, move out, Advanced connect (SYNC-047…) | thread (`gitsync/wizard`) | no: a provider call or a connection stopped half way leaves a token, key, repository or history behind | spinner and text on the wizard page and in Settings → Sync, Next off | **IS NOW** (JOB-033): the dialog's Cancel closes it, the call goes on and stays in the indicator (JOB-010) |
| Managed git: detect, install, check for a newer one, remove (SYNC-023…) | thread (`gitsync/install_ui.rs`) | install, update and the check: yes (flag, what was downloaded is removed); removal: no | spinner, text, a progress bar and Cancel | **IS NOW** (JOB-034) |

## Export

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| PDF export of a note or drawing (built-in, PDF-001) | **UI thread** (`run_export`) | no | nothing (the window waits) | planned: a thread and a task "Exporting PDF…" with Cancel between pictures |
| PDF export of a folder, a book (BOOK-010) | thread | yes, between notes | spinner and "Laying out the book: 3 of 12 notes" with Cancel | **IS NOW** (JOB-013): "Exporting “Handbook” to PDF…" |
| PDF export through LaTeX (TEX-016) | thread (compile) | yes (flag ends the process) | spinner "Compiling with LuaLaTeX… Cancel stops it." | planned: a task; Close keeps it running |
| Export to Word (DOCX-002) | thread (pandoc) | yes (flag ends the process) | "Exporting to Word… Cancel stops it." (no spinner) | planned: a task and a spinner |
| `shale --export-pdf` (BOOK-011) | the main thread, no window | no | the terminal | out of scope (no window) |

## Import

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Import dialog: converting Word files with pandoc (IMP) | thread | yes (flag) | "Converting the Word files with pandoc…", Import off | planned: a task; Close lets a started import finish |
| Import of mails, events, files into notes (`import_action`) | UI thread | no | nothing (usually quick) | planned: a thread when it is more than a few files |
| Mail import from the mail view (MAIL-070) | thread (`app/mail.rs`) | no | nothing until the dialog opens | planned: "Reading the mails…" |
| Import of saved web pages from the spool of the browser extension (CLIP-009, CLIP-067, #427) | none yet | no (a half-written clip is worse than a late one; the rest waits in the spool) | not built | planned with the clipper: a task "Importing saved web pages…" on a worker thread (SHOULD BE, CLIP-067) |
| Pandoc probe (settings); TeX probe | thread | no | "Looking for TeX on this computer…" | TeX probe: **IS NOW** (JOB-035, quiet); pandoc probe: planned, listed only after 2 s |

## TeX, updates

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| TeX install, update, removal (managed TeX Live, CTAN packages, TEX) | thread (`pdf/tex/install`) | install and update: yes (handle); removal: no | spinner, progress bar and Cancel in Settings and the setup dialog | **IS NOW** (JOB-035) |
| TeX test compile, test page of a set and template set import (`tex/settings_ui.rs`, `sets_ui.rs`) | thread | import: yes (handle); the two compiles: no (the compile does not look at the flag) | spinner and text, progress and Cancel for the import | **IS NOW** (JOB-035) |
| Update check, download, install (UPD, `update/mod.rs`) | thread, one `cancel` flag for all three | check and download: yes; install: no (a few file operations that undo themselves on an error) | spinner next to "Check for updates now"; the notice shows the text with a spinner and Cancel | **IS NOW** (JOB-036): "Checking for updates…", "Downloading Shale ‹version›…", "Installing Shale ‹version›…" |

## Mail

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| Opening the mail store (`MailStore::open`, MAIL-050; the integrity check, MAIL-006, and a possible index rebuild) | thread, started with the first frame of the window when an account is set up (else when the mail view first opens) | no | a spinner and "Checking the mail index…" in the view ("Opening the local mail copy…" in the instant between) | **IS NOW** (JOB-015) |
| Rebuilding the mail index (USE-012) | the store's writer thread | no | "Rebuilding the mail index from the message files…" | planned: a task |
| Mail sync (IMAP, MAIL) | thread per account (`imap_sync::start`) | yes (flag) | progress in the view, "Cancel sync" | planned: a task per account |
| Mail: loading a listing, reading and downloading a message, search, send, sign-in (OAuth), key and GnuPG work | threads of the mail UI (`mail/ui.rs`, `send.rs`, `oauth.rs`) | sign-in: flag and timeout; the others: no | per-view lines ("Loading the list from the server…", "Reading the message…", "Searching…") | not tasks (they belong to their view); send and sign-in planned |
| Mail watch (IDLE, `mail/watch.rs`) | a thread of its own for the session | yes | nothing | not a task: it runs for the whole session |

## Calendar and feeds

| Action | Runs today | Stop | Shows today | Registry |
|---|---|---|---|---|
| CalDAV load and sync of a calendar (CAL) | thread (`calendar/store.rs`) | no | status text "syncing…" in the sidebar and dialog, spinner | planned: a task per calendar: "Syncing the calendar “Work”…" |
| CalDAV account discovery (`calendar/dialog.rs`) | thread | no | "Looking for calendars…" with a spinner | planned |
| Social feeds: refresh, load more, save a post (SOC) | thread (`social/state.rs`) | no | spinner on "Check now", "Save to vault", "Load more in between" | planned: "Checking the feed of @name…" |
| Social accounts: sign-in, test, secrets (`social/accounts_ui.rs`) | thread | no | "Talking to the server…" | planned |
| Social images (`social/images.rs`) | worker threads | no | a placeholder | not a task: it belongs to its picture |

## How a planned row is done

Each follow-up issue moves its actions the way JOB-011 to JOB-013 did: a `Key` constant and a text, the
start through `Busy::spawn` / `spawn_cancellable` (or `Busy::begin` for work that has a thread of its
own), the buttons through `busy::ui::button`, the spinner through `busy::ui::status`, Cancel where the
"Stop" column says yes, and a test per action: the button is off while it runs, Cancel stops it, the
indicator lists it and drops it.

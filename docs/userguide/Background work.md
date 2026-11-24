---
features:
  - "Busy states: spinner, what is happening, Cancel"
---
![[busy-indicator.png]]

Back to [[Start here]]. Whatever Shale does that takes longer than a moment says what it is doing,
and you cannot start it twice by accident.

# What you see

- **A spinner and a phrase** next to the button or in the dialog: "Syncing with git…", "Checking
  file names and links…", "Exporting “Handbook” to PDF…".
- **A vault is opened in the background.** At start, or when you open or create a vault, the start screen
  shows a spinner and "Opening ‹folder›…" with **Cancel** while Shale reads the notes; the window with
  your notes appears when it is done. Cancel drops the load and you can pick another vault. A vault that
  cannot be read says why on the start screen.
- **The mail index is checked as Shale starts** when you have a mail account, even if you have not
  opened the mail view: "Checking the mail index…" shows in the indicator. Open the view meanwhile
  and it shows the same text with a spinner until the check is done.
- **The mail index can be rebuilt** (Settings, Data usage, Mail index, **Rebuild**): "Rebuilding the mail
  index…" shows in the indicator and, with a spinner and **Cancel**, in the mail view. Cancel stops
  between two folders: every folder is either built again or as it was, so no mail seems to be
  missing; rebuild again to go on with the rest.
- **Each mail account's sync is a task of its own**: "Syncing ‹account›…" shows in the indicator and, with
  a spinner and **Cancel**, in the mail view for every account that is syncing. Cancel stops that account
  between two folders or two batches of messages, never in the middle of a message, so nothing is half
  saved; the view says "Sync stopped" and the next sync goes on with what is missing. The other accounts
  keep syncing. Waiting for new mail (IDLE) is not a task.
- **Sending mail is a task per account**: "Sending mail from ‹account›…" shows in the indicator and, with a
  spinner and **Cancel**, in the mail view above the account's name while its mails go out. A mail that is
  already on its way is never torn down (it could be half sent), so Cancel lets it finish, says "Stopping…"
  meanwhile, and keeps the mails after it waiting in the Outbox; nothing is sent again by itself until you
  queue, retry or send one again. A mail the server refuses stays in the Outbox as failed with the server's
  words, and the view says "A mail was not sent: …".
- **Calendars** ([[Calendar]]): every CalDAV calendar that loads is a task of its own, "Syncing the calendar
  “Work”…", with its text and **Cancel** under the calendar's line in Settings → Calendar → Calendars…, and "Find
  calendars" shows "Looking for the calendars of ‹server›…". Cancel takes effect at the next request, the
  calendar then keeps what it showed ("sync stopped") and the next sync loads it normally. The timer's load
  shows in the indicator only when it takes longer than two seconds; a load you ask for ("Sync now", a new
  calendar) shows at once. Closing the calendar dialog stops the look for calendars.
- **Feeds** ([[Feeds]]): the first page of a feed ("Loading “Home” of @name…"), the next page while you scroll
  (listed after two seconds), "Check now" and the timer's look for new posts ("Checking the feed of @name…",
  the timer's listed after two seconds), "Load more in between" and **Save to vault** ("Saving the post of
  @name…") are tasks, each with **Cancel** in the indicator (the feed also shows it where its spinner was). A
  stopped page keeps the list as it was and offers Retry; a stopped check is not a failure; a stopped save
  writes nothing. Signing in to a feed account in **Accounts…** is a task too, with **Cancel** in the dialog:
  nothing is stored for a sign-in that is stopped, and closing the dialog stops it.
- **Signing in to a mail account in the browser is a task per account**: "Signing in ‹account› in the browser…"
  shows in the indicator, in the mail view above the account's name, and as "Waiting for the browser…" in the
  account dialog, each with **Cancel**. Cancel, or closing the account dialog (Done, Cancel, Esc), stops
  waiting at once and frees the local address the browser answers to; a second sign-in of the same account
  does not start while one waits. A sign-in is kept whole or not at all: if Cancel comes before Shale has
  saved it, nothing is saved, and an earlier sign-in stays as it was.
- **Things that happen on their own are listed only when they take long.** Reading the vault folder for
  changes made by other programs ("Reading the vault folder…", on a slow network share), learning which
  words belong to your tags ("Learning the words of the tags…", **Cancel** stops it) and counting what Shale
  keeps on this computer show in the indicator after 2 seconds, not before: a quick one is nothing to look at.
- **Files you drop on the window** ("Reading the dropped files…"), **media copied into the vault** ("Copying
  media into the vault…", **Cancel** stops it before the next file; the links that were copied stay changed,
  the others stay as they were), a **file opened with another program** ("Opening ‹name›…", one task per
  file), the **Clear** and cleanup of a cache in [[Data usage and caches]] ("Clearing the social pictures…")
  and the look for pandoc and for TeX in Settings all show what they do the same way.
- **The buttons that would start the same thing again are off** while it runs. Point at one and it
  says what is still running. A key or the timer cannot start it a second time either.
- **Exports and the Word import** are listed too: the PDF export of a note or drawing (the window no longer waits
  for a long note with many pictures), the export through LaTeX, the export to Word and the conversion of
  Word files in the Import dialog. Each shows a spinner in its dialog and a line in the indicator.
- **Cancel** stands next to the spinner where the work can be stopped, for example the check of file names
  (Settings → Vault), the exports, the conversion of Word files, the install, update and import of Shale's own git and TeX Live and of template sets, and the check and download of an update of Shale itself (a cancelled download is removed). A PDF that is stopped is never written
  half way: nothing appears in the folder. A git sync has no Cancel: stopping it between
  saving your notes and sending them would leave the repository half way. It is quick, and it finishes.

- **Quick housekeeping stays quiet.** The versions Shale keeps of your notes (see [[File history]]), the look for
  git or TeX and the daily check for updates run often and are over in a moment: the indicator lists them only when one takes
  longer than two seconds. The page that shows them (the history panel, Settings) shows them at once.
- **The "Turn on sync" steps** (signing in, making a key, creating the repository, asking the server) show what they do on the
  step. They have no Cancel, because a call stopped half way could leave a token or a repository behind; the dialog's own
  **Cancel** closes the dialog and the call ends by itself.

# The indicator at the top right

While anything runs in the background, a small indicator sits at the top right of the editor, in the
row of the tabs. With several panes it is at the right of the rightmost one.

- With one task it shows the spinner and its text; with several, "2 tasks running".
- **Point at it**, or **click it** to keep it open (click again to close it): it lists every task with
  how long it has been running, and a **Cancel** button for the ones that can be stopped. A task
  that was cancelled shows "Stopping…" until it has ended.
- It is gone when nothing runs.

**Closing a dialog stops its work.** An export dialog and the Import dialog save or write the result themselves,
so Cancel (or Esc, or a click outside) stops the work and the task leaves the list when it has ended. In an
export dialog the first Cancel stops the running export and a second one closes the dialog. Stopping from the
indicator leaves the dialog open and says why.

[[Git sync]] has the git side, [[PDF]] the export, [[Vault settings]] the check of file names.
The search over all notes runs in the background too: a search that takes longer than two seconds shows
as "Searching…" with Cancel (the last list stays). Not every long action is on the indicator yet; they
are moved one by one.

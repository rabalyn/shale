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
- **The buttons that would start the same thing again are off** while it runs. Point at one and it
  says what is still running. A key or the timer cannot start it a second time either.
- **Cancel** stands next to the spinner where the work can be stopped, for example the check of file names
  (Settings → Vault) and the PDF export of a folder. A git sync has no Cancel: stopping it between
  saving your notes and sending them would leave the repository half way. It is quick, and it finishes.

# The indicator at the top right

While anything runs in the background, a small indicator sits at the top right of the editor, in the
row of the tabs. With several panes it is at the right of the rightmost one.

- With one task it shows the spinner and its text; with several, "2 tasks running".
- **Point at it**, or **click it** to keep it open (click again to close it): it lists every task with
  how long it has been running, and a **Cancel** button for the ones that can be stopped. A task
  that was cancelled shows "Stopping…" until it has ended.
- It is gone when nothing runs.

[[Git sync]] has the git side, [[PDF]] the export, [[Vault settings]] the check of file names.
Not every long action is on the indicator yet; they are moved one by one.

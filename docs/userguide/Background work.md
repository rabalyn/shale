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
- **The buttons that would start the same thing again are off** while it runs. Point at one and it
  says what is still running. A key or the timer cannot start it a second time either.
- **Cancel** stands next to the spinner where the work can be stopped, for example the check of file names
  (Settings → Vault), the PDF export of a folder, the install, update and import of Shale's own git and
  TeX Live and of template sets, and the check and download of an update of Shale itself (a cancelled
  download is removed). A git sync has no Cancel: stopping it between
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

[[Git sync]] has the git side, [[PDF]] the export, [[Vault settings]] the check of file names.
Not every long action is on the indicator yet; they are moved one by one.

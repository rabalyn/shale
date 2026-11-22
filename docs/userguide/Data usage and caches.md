---
features:
  - "Data usage and caches"
---
![[data-usage.png]]

Settings → Data → **Data usage…** (or Ctrl+Shift+D) shows what Shale keeps
on disk. Back to [[Start here]].

- **Saved**: the vault by kind (notes, drawings, pictures, PDFs, other files, `.shale/`,
  `.trash/`, `.git/`), its 10 largest files and folders, and your data outside the vault: mail
  per account, drafts, outbox, calendars, recovered notes and, for a vault that is not a git
  repository, its file history ([[File history]]; in a git repository it is in `.git/`, counted with the
  vault). When the notes link pictures, PDFs, sounds or videos from outside the vault
  ([[Sounds and videos]]), **Outside media linked by this vault** shows how many such files there are
  (each once) and their size, and in red how many are not found or not set up on this computer; the
  files are not in the vault and not synced. Nothing here can be deleted from this window.
- **Caches** are what Shale can build or fetch again: social pictures, the social post index,
  the mail index and its backups, damaged copies of the mail index, learned tag words, the
  keyboard debug log and the undo histories of notes (vim `u` / Ctrl+R; for a vault in a cloud-synced or network folder they are kept in Shale's data folder, not in the vault). Each row shows its size and has a **Clear** button.
- How long each cache is kept is a setting (on its row, and in Settings under "How long caches
  are kept"): social pictures 30 days and 500 MB, social posts 365 days and 10 000 per account,
  the newest 3 mail index backups, damaged copies forever, the debug log 10 MB, undo steps of notes 30 days. Every value can
  also be "forever" or "no limit". The keys are in [[Settings reference]] (`caches`).
- A cleanup runs a minute after the start and then daily. It never touches your notes, files or
  mail.

#userguide #data

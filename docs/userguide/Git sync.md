---
features:
  - "Git sync"
---
# Git sync

Keep a vault on several computers with git. Back to [[Start here]].

- Needs `git` installed. In Settings → General → Git sync, enter the URL of a git repository
  (`https://…`, `ssh://…`, `git@host:notes.git` or a folder path) and press **Connect**, or run
  `git init` in the vault folder yourself. An address with a password in it, and plain `http://`
  or `git://` (except on this computer), are refused: use a credential helper or an SSH key.
  Shale never asks for a password; that is git's business.
- Shale then syncs on its own: when the vault opens, every few minutes (default 5; the setting
  is `sync.interval_minutes`) and on **Sync now**. "Sync this vault automatically" switches it
  off (`sync.enabled`). Those settings are in the vault's `.shale/settings.json`, so they travel
  with it ([[Vault settings]]).
- A sync commits every change (the `.shale/` files too, so settings and open tabs follow you),
  fetches, merges and pushes. Commit messages read `Sync from <computer>: 3 file(s)`, with an
  optional prefix and suffix. Without a remote, Shale only keeps a local history.

## Nothing is ever lost

- A note changed on two computers keeps the remote version, and yours is kept next to it as
  `Note (conflict 2026-11-14 1530).md`. A note deleted on one side and changed on the other is
  kept.
- Shale never force-pushes, never writes conflict markers into notes, and pauses if git is in
  the middle of something you started (a merge, a rebase).
- Edits you type while a sync runs are never overwritten: your text is saved as a conflict copy
  next to the new version.

This guide's own `.shale/settings.json` has sync switched off, because it lives inside the
Shale repository.

#userguide #git

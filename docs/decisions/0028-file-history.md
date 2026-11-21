# ADR-0028: File history: snapshot commits and restoring a version

- **Status**: Accepted for the implementation of #471; the points under "Open for the owner" are reversible defaults
- **Date**: 2026-11-21
- **Specs**: SYNC-030…SYNC-046, CFG-066, USE-025 ([`docs/spec/vault-and-files.md`](../spec/vault-and-files.md)); SYNC-006 changes
- **Issues**: #471 (this), #467 (umbrella); #472 (the history panel) and #469 (the sync wizard) call what this record defines
- **Builds on**: [ADR-0005](0005-where-state-lives.md) (what is synced and what is device-local), [ADR-0008](0008-git-sync.md) (git sync), [ADR-0027](0027-managed-git.md) (the selected git), `docs/storage-on-shares.md` (inputs for #471)

## Context

The owner decided in #467: every vault gets a history of its notes, with or without a git server. The
history is git commits (snapshots) made on save and when idle, and a restore. A vault that syncs with a
git server keeps its `.git` in the vault (as today). A vault without one keeps its repository outside the
vault. The panel that shows the versions is #472; this record and #471 make the engine and the actions it
calls.

## Decisions

1. **Where the history lives.** A vault with `<vault>/.git` (server sync, or a repository it already was)
   gets its snapshots in that repository. A vault without `.git` gets a repository whose git directory is
   `<data folder>/shale/history/<vault id>` and whose work tree is the vault. Git runs with
   `--git-dir=<…> --work-tree=<vault>` (an argument list, SYNC-001 still removes the `GIT_*` variables).
   **No `.git` file or folder is ever written into the vault**: a gitfile pointer would travel through
   Syncthing, Nextcloud or Dropbox and break on the other devices.
2. **The vault ID** is 128 random bits in hex in `<vault>/.shale/history-id` (one line). `.shale/` is
   synced except `state.json` and `undo/` (ADR-0005, SYNC-006, #476), so the same vault on two devices has
   the same ID and each device keeps its own history in its own data folder. A vault folder that is moved
   or renamed keeps its history: the git directory holds a file `worktree` with the folder it belongs to;
   when that folder no longer exists, the new place takes over. When it still exists and has the same ID
   (the folder was copied), the copy gets a **new ID** and starts its own history.
3. **Turning on.** The global setting `history.enabled` is on by default for every vault (CFG-066:
   `[history]` with `enabled`, `coalesce_minutes`, `idle_seconds`; old files load with the defaults). A
   vault with `.git` and sync turned off still gets snapshots. Without a usable git (SYNC-003) history is
   off and Settings → Sync says why.
4. **What local history tracks.** Notes and drawings (VAULT-002: `*.md`, `*.excalidraw`,
   `*.excalidraw.md`, `*.canvas`), not media, so the data folder stays small. In a vault repository a
   snapshot stages only the paths it is about; everything else is still committed by the sync (SYNC-006).
   Never snapshotted: `.shale-tmp-*`, the temporary and housekeeping names of cloud clients and shares
   (`.syncthing.*.tmp`, `.stfolder`, `.stversions`, `.stignore`, `.~lock.*#`, `~$*`, `*.tmp`, `*.part`,
   `.nfs*`, `.fuse_hidden*`, `*.icloud`, `desktop.ini`, `.DS_Store`, `Thumbs.db`), `.shale/undo/` and
   `.shale/state.json`. Local history does not track `.trash/` (a trashed note is a deletion of its path).
   Content is read by git from the file (never trusted by time, NET-003).
5. **Snapshots.** A save of a note or drawing marks its path. The path is committed when it has not been
   for `coalesce_minutes` (default 10): at most one snapshot per note per window while typing; or when its
   buffer has had no change for `idle_seconds` (default 60); or when its last tab closes; or at window
   close (best effort, bounded wait of 3 s). Only saved text is committed, never a buffer. A snapshot runs
   `git add -A -- <paths>` and `git commit --only -- <paths>`, so it never takes other staged changes along,
   makes no fetch and no push, and makes no commit when the paths equal HEAD. It runs in a background job
   (JOB-001) one at a time, never while the sync job runs, and takes the tree lock (SYNC-013) for the
   time it runs. When it cannot get the lock quickly, or the repository is busy (`index.lock`, a merge in
   progress, a detached HEAD), it is retried later and never blocks typing. A new local history first
   makes one snapshot of the vault as it is (reason `initial`).
6. **Message.** A short subject and git trailers: `Shale-Reason:`
   `save|idle|close|initial|sync|restore|merge|rename|delete`, `Shale-Device:` (the computer's name,
   `gitsync::host_name()`), one `Shale-Path:` per path, `Shale-Restored:` (the restored commit) for a
   restore. The sync's commits carry `Shale-Reason: sync` and `merge`, with the subject and the
   `commit_prefix`/`commit_suffix` settings as before. `history::parse_message` reads them and tolerates
   commits without them (made by other tools).
7. **Sync pushes snapshots as they are**: no squash, no history rewrite (SYNC-012). Cost: a snapshot is a
   small text delta; a busy writing day gives some dozens of commits per note (at most 6 an hour while
   typing, plus idle ones), and git packs delta-compress them, so the server grows by about the size of the
   edited text, a few KB per snapshot. "Squash on push" (one commit per day on the server) is **rejected**:
   it rewrites what the local history shows and needs a force push. Open for the owner below.
8. **Restore never resets or rewrites.** "Restore this version" first snapshots the current saved text, then
   makes the old text the note's new content as one undoable edit when the note is open (the undo history
   of EDIT-021, one step) or an atomic write through the usual save path when it is not, and then makes a
   snapshot with reason `restore` and `Shale-Restored:` naming the commit. "Copy text from this version"
   returns the text. "Open this version read-only" shows the old text in a read-only view (a dialog with
   the text, a copy button), because no tab can hold text that is not a file; #472 may do better.
   **A deleted note** is listed from the commits that deleted a path (renames are not deletions) and is
   written back to its path, or to a free name `Name (restored).md` when the path is taken.
9. **Renames** make a snapshot with reason `rename` (the old and the new path, so git sees the move) from the
   rename and move actions; the versions of a note follow its renames with `git log --follow`.
10. **Query API** (module docs of `gitsync::history`): versions of a note (commit, time, reason, device,
    lines added and removed or the `binary` flag, the path at that version), the text at a version, the
    deleted notes, the size of the history on disk. Binary files and LFS pointers have no counts (git's
    numstat prints `-`).
11. **Retention: keep everything.** Thinning (hourly after a day, daily after a month) would rewrite history,
    so it is not done; follow-up issue. The size of a local-only history is shown in Data usage (USE-025);
    an in-vault `.git` is already counted as vault data (`.git/`, USE-002).
12. **Entry points until #472.** A note's ⋯ menu has "Versions of this note…", the file tree menu of the
    vault and Settings → Sync have "Restore a deleted note…"; both open a plain list dialog that #472
    replaces. They are also definable key actions without a default key (KEY-*).

## Rejected

- A gitfile or `.git` link in the vault (breaks on synced folders); one shared repository for all vaults
  (one broken vault ruins the others); thinning by rewriting; an embedded git library (owner decision).

## Open for the owner

Defaults implemented now, each reversible:

- Local history does not track media (only notes and drawings).
- Old snapshots are never thinned out (follow-up issue "thinning").
- A copied vault gets a new ID and a history of its own.
- A vault that later gets `.git` continues in that repository; the old local history is not merged in
  (it stays in the data folder and is shown in Data usage until removed).
- A new local history starts with a snapshot of the vault as it is.
- History is on by default.
- The sync pushes snapshots without squashing them.

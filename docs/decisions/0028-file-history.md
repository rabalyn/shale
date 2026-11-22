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
    deleted notes. Binary files and LFS pointers have no counts (git's
    numstat prints `-`).
11. **Retention.** A history in a vault's own `.git` is kept as it is (no thinning: it is pushed, SYNC-012).
    A local-only history is thinned, see the amendment of 2026-11-22 below (SYNC-067…SYNC-071). The size of
    a local-only history is shown in Data usage (USE-025); an in-vault `.git` is already counted as vault
    data (`.git/`, USE-002).
12. **Entry points until #472.** The ⋯ menu of a note or drawing has "Versions of this note…" and Settings →
    Sync has "Restore a deleted note…"; both open one plain list dialog that #472 replaces. There is no
    command palette in Shale, and no key action was added (they are part of the keyboard reference and its
    tests); #472 may add them.

## Rejected

- A gitfile or `.git` link in the vault (breaks on synced folders); one shared repository for all vaults
  (one broken vault ruins the others); thinning a history that is pushed (a rewrite needs a force push); an embedded git library (owner decision).

## Open for the owner

Defaults implemented now, each reversible:

- Local history does not track media (only notes and drawings).
- Thinning: see the amendment below (decided by the owner, #479).
- A copied vault gets a new ID and a history of its own.
- A vault that later gets `.git`: decided, see the amendment of 2026-11-22 (#485) below.
- A new local history starts with a snapshot of the vault as it is.
- History is on by default.
- The sync pushes snapshots without squashing them.

## Amendment 2026-11-22: thinning a local-only history (#479)

Owner decision: thin **only** local-only histories (the git directory in the data folder). Every snapshot
is kept for a day, hourly ones for a month, daily ones after that. A vault's own `.git` is never rewritten
(SYNC-012: no history rewrite, no force push). This replaces "keep everything" of item 11 and the open
default above.

- **Which commits stay** (`history::keep`, pure: commits newest first with time and message, plus "now"):
  younger than 24 h: all; 24 h up to 30 days: the newest of each hour; older: the newest of each day; always
  the newest commit and the root ("Start of the history"). **Buckets are UTC**: the same result on every
  machine and in every test, no daylight-saving hours with 0 or 2 slots; the cost is that a "day" starts
  at another hour than the person's midnight, which nobody sees in a thinned history older than a month.
- **Never dropped**: snapshots with `Shale-Reason: restore`, `delete` or `rename` (a restored version
  must not vanish, a rename keeps `git log --follow` working), every commit that deletes a path (the sync's
  and other tools' too), and **the commit before a deleting commit**: the deleted note's last text is the
  parent of the deleting commit (`deleted_content`), so without the parent a note would come back with an
  older text or not at all. The extra commits are few (one per deletion).
- **The rewrite** (`gitsync::history::thin`): under the tree lock, read `main` (the old tip) and its
  first-parent commits; build a new linear history oldest first with `git commit-tree` (same tree, same
  message, same author and committer with their dates through the `GIT_AUTHOR_*` and `GIT_COMMITTER_*`
  variables of a `Git` clone, parent = the previous kept new commit). Then `git update-ref refs/heads/main
  <new tip> <old tip>`: **compare-and-swap**, so a branch that moved or any failure before leaves the old
  history whole (nothing was written to a ref until then; the new objects are unreachable and are removed by
  the next gc). Then `git reflog expire --expire=now --all` and `git gc --prune=now` (not `--auto`: `--auto`
  does nothing below its thresholds, and the point is that Data usage shrinks; the repository is small and
  the run is a background job once a day). No `filter-branch`, no new dependency. The vault's files are
  never touched. `Shale-Restored:` trailers name commits that may be gone; they are information, nothing
  reads them back.
- **When**: at most once a day per vault, 60 s after a vault opens and then every 24 h, only while no
  snapshot, sync or read runs (a read of an old commit would fail when gc removed it); the time of the last
  run is the file `thinned` in the git directory. No setting: the owner gave fixed rules (a setting can be
  added when someone asks). Settings → Sync says which rule applies.
- Not thinned: a vault with `.git`. A person who wants a small history there can use a local history
  (SYNC-049).

## Amendment 2026-11-22: connecting a server moves the local history into `.git` (#485)

Owner decision: when a server is connected to a vault whose history is kept on this computer, that
history is **moved** into the vault's `.git` (copy, check, remove: the reverse of the move-out of #469,
SYNC-063), so the versions have one source. This replaces the open default "continues in that repository,
the old local history is not merged in" (SYNC-072…SYNC-075).

- **When**: in the job that connects (the wizard's "asking the server" step, or Advanced → Connect), before
  `git init` and `origin`; only for a vault without `.git` whose git directory has a `HEAD`. Under the tree
  lock, with no snapshot, thinning, read or sync running.
- **How**: copy `<data>/shale/history/<id>` to `<vault>/.git.moving`; the refs and `HEAD` must equal and
  `git fsck` must pass; rename it to `.git`; `core.bare=false`, no `core.worktree`; `git status` must work.
  Any failure removes the copy and leaves the vault without `.git` and the local history untouched.
- **The old folder is removed right after the checked copy**, not kept as `<id>.moved` until the first sync
  succeeded. Reasons: the copy is verified and renamed atomically, the first sync never rewrites it
  (SYNC-012), so the history in `.git` cannot get worse by a failing sync, and a kept second copy would be
  shown in Data usage as a second history and could diverge (snapshots, thinning). The only thing a kept
  copy would protect against is a bug in the check itself.
- **The first sync** is the ordinary one: an empty server gets the history as it is (no rewrite); a server
  with content is merged with `--allow-unrelated-histories`, both histories stay in the graph.
- **After the move** the history is the vault's own repository: pushed as it is, never thinned (SYNC-069).
  `.shale/history-id` stays (a synced file); it names a history that is gone until the repository is moved
  out again.

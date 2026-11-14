# ADR-0008: Integrated git sync

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: SYNC-001…SYNC-015, CFG-005…CFG-008, VAULT-004
- **Builds on**: [ADR-0003](0003-data-safety.md) (never lose data),
  [ADR-0005](0005-where-state-lives.md) (vault settings travel with the vault)

## Context

The owner's decision on ADR-0005: vault settings stay in the vault so a vault behaves the same on
every machine, and the vault, `.shale/*` included, is synced by an **integrated git sync**:

- git must be installed on the system;
- the user either enters a remote repository URL in Shale's settings, or simply runs `git init`
  in the vault folder; Shale then syncs changes on its own;
- the sync interval is configurable;
- commit messages get a configurable prefix and/or suffix.

ADR-0003 is the top priority: a sync must never lose or corrupt a note. Git already keeps every
committed version, so the risks are in the working tree: a merge that leaves conflict markers in
a note, a forced push that drops another machine's commits, and an editor buffer that is saved
over a file the sync just changed.

## Decision (proposed)

1. **Git, not a library.** Shale runs the system `git` through `std::process::Command` with
   argument lists, never a shell, with stdin closed, `GIT_TERMINAL_PROMPT=0` (no password
   prompts that hang a background job), and `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE`
   removed from the environment so the vault is always the repository that is touched. The
   user's own git configuration (identity, credential helpers, SSH keys) is used. Without git
   the settings say so and nothing else changes.
2. **When it syncs.** Sync is active when `<vault>/.git` exists and the vault setting
   `sync.enabled` is on (default on). It runs when the vault opens, every
   `sync.interval_minutes` (default 5, at least 1), and on "Sync now". A repository in a parent
   folder of the vault is not used.
3. **Connecting a remote.** Entering a URL in the settings runs `git init` if there is no
   repository yet, then `git remote add origin <url>` (or `set-url` if `origin` exists). Only
   `https://`, `http://`, `ssh://`, `git://`, `file://`, scp-style `user@host:path` and
   absolute local paths are accepted; a URL that starts with `-`, contains whitespace or control
   characters, or uses a `transport::` helper (`ext::`) is refused. Without a remote, sync only
   commits (a local history of every change).
4. **One sync cycle**, in a background job:
   1. Refuse to start if a merge, rebase, cherry-pick or revert is in progress, or HEAD is
      detached. Shale never finishes or aborts what someone else started.
   2. Commit everything: `git add -A` (Shale's own temporary files `.shale-tmp-*` excluded),
      then commit if anything is staged. Message: `<prefix>Sync from <host>: <n> file(s)<suffix>`;
      prefix and suffix are used as written. If no git identity is configured, the commit uses
      `Shale <shale@localhost>` for that command only.
   3. `git fetch origin`. If the remote branch does not exist yet, push with `-u`.
   4. Bring the histories together: fast-forward if possible, otherwise
      `git merge --no-ff --allow-unrelated-histories` (unrelated histories happen when a vault
      that already has notes is connected to a remote that has notes too).
   5. **Conflicts become conflict copies, never markers.** For every unmerged path:
      - both sides have it: the remote version stays at the path, the local version is written
        next to it as `Name (conflict YYYY-MM-DD HHMM).ext` (`.excalidraw.md` stays one
        extension; a taken name gets a number). Both are committed and pushed, so every machine
        sees both.
      - one side deleted it, the other changed it: the changed version is kept. A sync never
        deletes a file that the other side changed.
      - `.shale/state.json` (session state that may be lost, ADR-0005): the local version wins
        and no copy is made.

      If anything in this step fails, `git merge --abort` puts the tree back to the local
      commit, which already holds every local change.
   6. `git push origin <branch>`. **Never** `--force` or `--force-with-lease`. If the push is
      rejected because the remote moved on, the next cycle fetches and merges again.
5. **Editor buffers and the sync.** Before a cycle the app saves every unsaved buffer. While the
   job changes the working tree (step 4–5) the app does not autosave. When the job ends it
   reports the files it changed: open notes and drawings without unsaved edits are reloaded; a
   buffer edited meanwhile is saved as a conflict copy next to the synced file (ADR-0003 rule),
   never over it. On exit the app waits for the working-tree step before its final save.
6. **Every result is shown**: a short status line in the settings (last sync time, or the error
   as git reported it) and a message when files were pulled or conflict copies were made.
7. **Settings** (`<vault>/.shale/settings.json`, synced with the vault): `sync.enabled`,
   `sync.interval_minutes`, `sync.commit_prefix`, `sync.commit_suffix`. The remote URL is git's
   own configuration (`git remote`), not a Shale setting, so `git remote set-url` and the
   settings field always agree.

## Consequences

- A vault works the same on every machine, and every change has a history.
- `.shale/state.json` is synced too (owner's decision), so open tabs follow the user from
  machine to machine; it never produces conflict copies.
- Conflict copies appear when two machines edit the same note between syncs. That is the
  intended trade-off from ADR-0003: a duplicate is better than a lost edit. The user merges them
  by hand.
- Shale commits on its own; a user who wants hand-made commits turns `sync.enabled` off for
  that vault.
- Credentials are git's job (credential helper, SSH agent). A remote that needs a password
  prompt fails with git's message instead of hanging.
- Large binary attachments go into git history. Git LFS is the user's choice and works because
  git does it.

## Alternatives considered

- *libgit2 / gitoxide*: no dependency on an installed git, but a large dependency, its own
  credential handling, and behaviour that differs from the `git` the user knows. Rejected; the
  owner accepts "git must be installed".
- *`git pull --rebase`*: rewrites local commits, and a conflict stops in the middle of a rebase.
  Merge commits are simpler to undo and never rewrite history.
- *Keeping conflict markers in the file*: breaks notes and drawings (JSON) for every reader.
  Rejected in favour of conflict copies.
- *Force push on rejection*: drops another machine's commits. Never.

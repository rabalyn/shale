# ADR-0029: The "Turn on sync" wizard, git credentials and detection of shared folders

- **Status**: Accepted and implemented in #469; the points under "Open for the owner" are reversible defaults
- **Date**: 2026-11-21
- **Specs**: SYNC-047…SYNC-066, NET-017 ([`docs/spec/vault-and-files.md`](../spec/vault-and-files.md))
- **Issues**: #469 (this), #467 (umbrella), #480 (the provider sign-in flows, open), #485 (local history into `.git`, open)
- **Builds on**: [ADR-0008](0008-git-sync.md) (git sync), [ADR-0027](0027-managed-git.md) (the selected git), [ADR-0028](0028-file-history.md) (file history), `docs/storage-on-shares.md` (the lists the detection uses)

## Context

Today a vault is synced by typing a remote URL into Settings → Sync. That asks a non-technical person to
know what a remote, a token and an SSH key are. #467 wants a guided **Turn on sync** with three ways to keep
notes safe (history on this device, a folder another app syncs, a git server), a sign-in that Shale can
store safely, and a warning when the vault is on a network share or in a cloud-synced folder.

## Decisions

1. **The wizard is a dialog of its own**, with a small state (`gitsync/wizard/`) owned by the sync state, not
   fields on `ShaleApp`. Settings → Sync shows **Turn on sync…** as the way in for a vault without a
   remote; the old URL field and "Connect" stay under **Advanced** and work as before (SYNC-004…SYNC-022).
   A vault that has `.git` keeps working unchanged.
2. **Three choices.** *History on this device only* and *My folder is already synced by another app* are the
   same storage (ADR-0028: the repository outside the vault); the wizard only turns `history.enabled` on
   and explains. *Sync with a server* makes `.git` in the vault as today.
3. **HTTPS with a token is the default sign-in.** The token is kept in the system keyring through
   `secrets.rs` (application name `shale-git`; entry = server `https://host[:port]` + user name), never in
   the settings file, never in `.git/config`, never in a remote URL, never on a command line. Without a
   keyring the wizard refuses to keep the token and recommends an SSH key (nothing is written to a file).
4. **The credential mechanism is `GIT_ASKPASS` pointing at the Shale executable in a helper mode.** Git asks
   `GIT_ASKPASS` for a password (the prompt names `https://user@host`); the helper is started as
   `shale <prompt>` with `SHALE_GIT_ASKPASS=1` in its environment, answers from the keyring on standard
   output and exits, without opening a window. Why this and not the alternatives:
   - A git *credential helper* (`credential.helper=…`) would be a program git runs through a shell
     (`!…`) or a `git-credential-*` program on `PATH`; the first breaks SYNC-001 (no shell), the second
     needs an installed file next to git.
   - `GIT_ASKPASS` is a plain program name, started without a shell, the same on Linux, macOS and Windows,
     and Shale's own executable is already there. It is set only on the `Git` Shale runs, so the person's git
     configuration is not changed (as ADR-0027 decided for the managed git).
   - The helper answers only for a prompt that names a host and user it has a token for; for every other
     prompt it prints nothing and exits 1, so git fails at once ("terminal prompts disabled") as before.
   - The person's own credential helpers are **not** cleared: git asks them first and only then the
     askpass program, so a vault that already syncs through the Git Credential Manager or
     `osxkeychain` keeps working. `GCM_INTERACTIVE=never` is set so that the Git Credential Manager
     fails at once instead of opening a window in a background sync.
   - A remote made by the wizard is `https://<user>@host/owner/repo.git` (a user name alone is fine,
     SEC-010), so the prompt names the user and the helper needs no second environment variable.
5. **SSH**: an existing key is used as it is; a new key is made with `ssh-keygen -t ed25519 -N ""` (an
   argument list, no shell) in a private folder `~/.ssh/shale_<host>` that never overwrites a file. The
   wizard shows the public key with a Copy button and the provider's "add key" page. A key without a
   passphrase is the price of a sync nobody has to unlock; the person may use their own key with one and
   the agent. The test sync of the wizard (only that) runs ssh with `StrictHostKeyChecking=accept-new`
   and `BatchMode=yes` (the first connection to a host is trusted and remembered; a changed key is refused).
6. **Repositories through the provider's API** (Forgejo and Gitea `/api/v1/user/repos`, GitHub
   `/user/repos`) with the token, sent as `Authorization: token …` in curl's configuration on standard input
   (SEC-002, SEC-017). GitLab has no create in the wizard (a link to the website). The HTTP client is the
   existing `curl` of `calendar/dav.rs`; there is no new dependency.
7. **The test sync is the real sync job** (`gitsync::sync`, started like every sync, JOB-001), preceded by
   one `git ls-remote` that tells an empty repository from one with notes and shows the plain-words
   result of a wrong token, no network or a missing repository before anything is merged.
8. **Detection is a pure function of injectable inputs** (`gitsync/detect.rs`): mount table text
   (`/proc/self/mountinfo`) and `statfs` magic numbers (Linux), `f_fstypename` (macOS), drive type and
   path (Windows), the environment and `info.json` files for Dropbox and OneDrive, marker files in the
   vault and its parents. It runs in a background job, only **warns**, and never decides anything:
   safety comes from keeping local-only history outside the vault, not from detection.
9. **A `.git` inside a detected folder** gets a warning. Where the vault has **no remote** it also gets an
   offer to move the repository out to the layout of ADR-0028 (copy the git directory to
   `<data>/shale/history/<id>.moving`, verify that `HEAD` and `git fsck` agree, rename it to `<id>`, and only then remove
   `<vault>/.git`). With a remote nothing is offered (the repository is the sync); the warning says why it is
   risky and what the person can do.

## Open for the owner

- **Device and OAuth sign-in (#480)**: needs one registered OAuth application per provider (client IDs
  Shale ships) and a decision for self-hosted servers. Not built; the token path is the default.
- **Keys without a passphrase** (decision 5) and **trusting a new SSH host on first use** are reversible
  defaults; the alternative is a passphrase field and an explicit host-key confirmation step.
- **Connecting a server to a vault that has a local-only history** starts the history of `.git` empty
  (ADR-0028 open point); moving the local commits into `.git` is not built (#485).
- **Without a keyring there is no token sign-in.** A session-only token would need the token in the
  helper's environment; the owner can decide that is acceptable.

# ADR-0027: A managed git, installed with one button

- **Status**: Accepted for the implementation of #468; the points under "Open for the owner" are reversible defaults
- **Date**: 2026-11-21
- **Specs**: SYNC-023…SYNC-029, CFG-054 ([`docs/spec/vault-and-files.md`](../spec/vault-and-files.md))
- **Issues**: #468 (this), #467 (umbrella: sync and file history for everyone); #469 (credentials, wizard) and #471 (snapshots) use the selection described here
- **Builds on**: [ADR-0008](0008-git-sync.md) (git sync), [ADR-0019](0019-pdf-export-through-latex.md) (the managed TeX Live, the model of this one), [ADR-0024](0024-gpl-licence.md)

## Context

Sync and file history run the `git` program (ADR-0008, SYNC-001); the owner decided in #467 that there is
no embedded git library. A person without git cannot turn sync on. The managed TeX Live (TEX-002, TEX-003)
shows the way to deal with a missing tool: detect the system one, offer a download after consent, build it
in a `.partial` folder, rename when complete, update and remove it by buttons, choose with Auto, System or
Managed. This record applies that to git and git-lfs and records what upstream actually offers per
platform (checked 2026-11-21 against the GitHub releases API).

## Upstream facts

| What | Release | Download | On disk | Checksum |
|---|---|---|---|---|
| MinGit 64-bit (zip) | git-for-windows `v2.56.1.windows.1` | 39.8 MB | 96.2 MB (378 files, `cmd/git.exe`, `ucrt64/`, `usr/`, `etc/`) | `4a92850b…dfefdafd` |
| MinGit arm64 (zip) | the same | 38.0 MB | about the same | `96571e44…b4cc8c` |
| git-lfs windows amd64 (zip) | git-lfs `v3.8.1` | 5.9 MB | 15.3 MB | `5bb771c1…defed9` |
| git-lfs windows arm64 (zip) | the same | 5.3 MB | about 14 MB | `7e70990e…ba4784` |
| git-lfs darwin amd64 / arm64 (zip) | the same | 6.2 MB / 5.6 MB | 15 MB | `4292b351…3b72` / `0ba10e89…2222c` |
| git-lfs linux amd64 / arm64 (tar.gz) | the same | 5.9 MB / 5.4 MB | 15.6 MB | `e72cf723…354b75` / `67895dd7…baec2d` |

- **MinGit** is the official portable build of Git for Windows (no installer, no registry, relocatable:
  `cmd\git.exe` finds its libexec relative to itself). The release page lists a SHA-256 for every asset
  and the GitHub API returns it as `digest: "sha256:…"`. It contains **no git-lfs**, so git-lfs is
  downloaded separately. It also contains `git-credential-manager` (decided in #469, not here).
  Installing it needs no administrator rights.
- **git-lfs** publishes zip (Windows, macOS) and tar.gz (Linux) archives with a top folder
  `git-lfs-X.Y.Z/` holding the single `git-lfs` binary (plus man pages and `install.sh`). Its release has
  `sha256sums.asc` (a PGP-signed list) and the API digests as well.
- **macOS: there is no official portable git.** Apple ships `/usr/bin/git` as a stub that starts the
  Xcode Command Line Tools (about 1 to 2 GB on disk, installed through a system dialog).
  Homebrew's bottles are not relocatable. git-scm.com links to installers (a `.pkg`) and third-party builds.
- **Linux:** upstream git publishes source only. Distributions package it; a portable static build exists only
  as third-party projects.

## Decision

1. **Where the files are.** `<data dir>/shale/git` (`dirs::data_dir()`, like `texlive`), built in
   `<data dir>/shale/git.partial`, renamed when complete. Layout: `cmd/git.exe` and the rest of MinGit
   at the top (Windows only), `lfs/git-lfs[.exe]` (git-lfs, every platform), and `shale-git.json`, a manifest
   (versions, tags, the program, the folders to add to `PATH`). The manifest is the single place that
   says what the folder is; the selection reads it, so the layout can change by platform.
2. **Per platform** (data in `gitsync/sources.rs`, replaced by a fake in tests):
   - **Windows:** MinGit (x86-64 or arm64) plus git-lfs, 45.7 MB download, about 112 MB on disk.
   - **macOS:** git-lfs only (6 MB). For git itself the button runs `xcode-select --install` and says that
     Apple's dialog installs the tools; Shale does not download anything for that.
   - **Linux:** git-lfs only, when a git exists and git-lfs does not. Without git the page shows the
     install command for the distribution (from `ID` and `ID_LIKE` of `/etc/os-release`) and a copy button;
     Shale does not run `sudo`.
3. **Pinned first install, upstream latest for updates.** The code pins version, size and SHA-256 of
   every file (the table above), so an install checks a hash that was reviewed with the code. The update
   check (opt-in, below) takes the newest release from the same projects and verifies it with the SHA-256
   that GitHub lists for the file in the same API answer.
4. **Selection** (`gitsync::managed::selected_git(choice, data dir, PATH) -> Git`): Auto uses the git of `PATH` if
   there is one, else the managed git; System only the one of `PATH`; Managed the managed one (where only
   git-lfs is managed, the system git with the managed git-lfs). The child gets `PATH` with the managed
   folders added (behind the system ones for Auto, in front for Managed); the user's own `PATH` is never
   changed. SYNC-001 holds unchanged (argument list, no shell, `GIT_TERMINAL_PROMPT=0`, no repository variables).
5. **Updates are opt-in.** `git.updates` is `Off` by default (no request), `Notify` or `Install`; checked
   30 seconds after the start and every 7 days, only when a managed install exists; `SHALE_UPDATES=off` wins.
   The update builds a whole new tree in `git.partial` and swaps by two renames; the old tree is
   removed afterwards, and put back when the swap fails. (A part that did not change is downloaded
   again; MinGit is 40 MB and releases are a few a year.)
6. **Credentials are not decided here.** The managed git is run with the same environment as before. #469 decides
   how a Shale-controlled credential helper is configured (for the managed git it would be passed with `-c`
   or a managed `gitconfig`, not by editing the user's configuration).
7. **Reuse.** The download (`download.rs`), the archive reader (`pdf/tex/archive.rs`, zip and tar.gz with the
   path rules of the template import), the process runner (`pdf/tex/proc.rs`), the work folder, `rename_noreplace`
   and the remove guard (`fsutil::removable_folder`, which TEX-003 uses too) are the TeX ones. The progress
   handle is a small type of its own, since the TeX one is bound to TeX's stages.

## Licensing

git is GPL-2.0-only, git-lfs is MIT, MinGit bundles MSYS2/MinGW components under their own licences (the
archive carries `LICENSE.txt`, which stays in the managed folder). Shale downloads the files from the upstream
projects on the person's request and does not bundle or redistribute them; Shale itself is GPL (ADR-0024). Running
git as a separate program is not linking.

## Consequences

- A person without git on Windows gets sync with one button and no administrator rights; on macOS and Linux
  the page names the one step that is needed (Apple's dialog, the distribution's package) and then adds git-lfs
  by itself.
- The pinned table must be refreshed when a pinned version has a security fix; the update check covers people who
  turned it on. A pin that is too old is a known cost (git CVEs are the reason for the opt-in updates).

## Open for the owner

- **A hosted relocatable git for macOS and Linux.** Chosen default: none (Xcode tools, distribution package).
  A build we host would need a build pipeline, a place to host it, and the duty to follow git's security releases.
- **Gatekeeper and signing on macOS.** git-lfs for darwin is not notarized; a file written by curl and
  unpacked by Shale carries no quarantine attribute, so it runs, but this has not been run on a real
  Mac (like UPD-016). A hosted build would have to be signed and notarized.
- **Trust of an unpinned update.** The SHA-256 of the API answer proves that the file arrived as GitHub
  has it, not who made it (the same limit as UPD-022). Verifying git-lfs's PGP-signed `sha256sums.asc` with
  a key built into Shale would improve this.
- **MinGit versus the full Git for Windows.** MinGit has no `git gui`, no bash. Sync needs none of it.

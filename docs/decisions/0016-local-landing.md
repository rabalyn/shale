# ADR-0016: Changes land locally; CI checks `main` afterwards

- **Status**: Accepted (owner decision, 2026-11-16)
- **Date**: 2026-11-16
- **Specs**: FORGE-002, FORGE-010, FORGE-011, BUILD-002, BUILD-004, DEV-011
- **Issue**: Forgejo raba/shale#215
- **Amends**: [ADR-0013](0013-forgejo-and-issue-tracking.md) §2

## Context

- One developer works on Shale, with several agents, on one machine. Forgejo has one CI runner.
- Every pull request ran the full CI (build, tests, coverage, audit, env-tests, about 10
  minutes) before it could be merged, and every merge moved `main`, so the next PRs often had to
  merge `main` again and wait for another run. On 2026-11-16 eight runs were queued (about 80
  minutes) before the first of seven ready PRs could be merged.
- The same checks take one to two minutes on the development machine with a warm build folder.
- What the runner adds over a local run: a clean container, the release profile
  (`cargo test --release`), coverage, `cargo audit`, unused dependencies, and the mirror of a green
  `main` to GitHub for the macOS and Windows tests (BUILD-016). On 2026-11-16 it caught two
  things the local runs had not: a test that only held in a debug build, and a test that depended
  on whether `git-lfs` is installed.

## Decision

1. **Changes land locally.** `scripts/land.sh <branch>…` merges each branch into the current
   `origin/main` in a worktree of its own, runs fmt, clippy `-D warnings`, `cargo test`, the commit
   check of FORGE-008 and, when CalDAV or mail code is touched, the podman real-server suites, and
   pushes `main` only when all pass. A lock makes agents land one at a time (FORGE-011).
2. **Conflicts are resolved in the branch**, docs and code alike, and then landed like any
   change (FORGE-010).
3. **CI runs on `main`, tags, the weekly schedule and by hand**, not on branches or pull
   requests. It is a check after the fact in a clean container, and it still mirrors a green
   `main` to GitHub. A red `main` is fixed first (FORGE-002).
4. **Pull requests are optional**: a record or a way to show a change; issues stay the one
   to-do list (ADR-0013 §4, §5).

## Consequences

- A change is on `main` minutes after it is ready, not after the queue.
- A failure that only shows in a clean container or the release profile is found after the
  landing, not before; `land.sh --release` closes the release gap when a change touches
  `cfg!(debug_assertions)` or build profiles.
- `main` holds merge commits made locally. Forgejo marks an open PR as merged only if its
  "autodetect manual merge" setting is on; otherwise the PR is closed by hand with a comment.
- The landing worktree and its build folder live in `<git dir>/land/` (several GB).

## Alternatives

- **Keep CI on every PR** (ADR-0013 §2 as it was): one check in a clean container before each
  merge, at the cost of the queue.
- **A second runner**: halves the queue, needs another machine or VM.
- **Merge without any check and rely on CI on `main`**: fastest, but `main` would be red often.

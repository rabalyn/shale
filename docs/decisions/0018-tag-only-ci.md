# ADR-0018: CI runs for tags only; check tags and releases are made by script

- **Status**: Accepted (owner decision, 2026-11-17)
- **Date**: 2026-11-17
- **Specs**: FORGE-002, FORGE-003, FORGE-011, FORGE-013, FORGE-014, BUILD-002, BUILD-012,
  BUILD-013, BUILD-016, DEV-011
- **Issue**: #274
- **Amends**: [ADR-0016](0016-local-landing.md) §3, [ADR-0017](0017-github-issues-forgejo-ci.md)

## Context

- Several Opus sessions work on GitHub issues in parallel, each with Sonnet subagents for the
  implementation, and land their branches with `scripts/land.sh` (FORGE-011), which already runs
  fmt, clippy for Linux, macOS and Windows, `cargo test` and, when needed, the podman suites.
- Forgejo has one runner. A full CI run for every push to `main` (about 10 minutes) queued up
  behind a busy day of landings and checked commits that were already checked locally.
- `land.sh` pushed `origin/main` but never moved the local `main`; it moved only with a manual
  `git pull`, so new sessions could start up to a dozen landings behind.
- `make-release.sh` was named in CLAUDE.md and the docs but did not exist.

## Decision

1. **Forgejo CI runs for `v*` tags only** (and the weekly schedule and a run by hand), not for a
   push to `main`. `land.sh` is the check before `main`.
2. **`land.sh` fast-forwards the local `main`** after it pushed `origin/main`, when the checkout
   of `main` is clean and has no commits of its own; otherwise it says why it left it.
3. **A check tag is a pre-release** `vX.Y.Z-rc.N`, made by `scripts/make-tag.sh` when the owner
   is done for now: it bumps the version, lands the bump, tags the landed commit and pushes the
   tag to Forgejo. A green run pushes `main` to GitHub; the tag stays on Forgejo, and GitHub
   builds nothing.
4. **A release is `vX.Y.Z`**, made by `scripts/make-release.sh`: it bumps the version, moves
   `[Unreleased]` and `docs/changelog-pending/` into the version's `CHANGELOG.md` section, runs
   the release check, lands, tags and pushes the tag to Forgejo. A green run pushes `main` and the
   tag to GitHub, which builds and publishes the release (BUILD-012).
5. A tag that was pushed is never moved or deleted: a failed run is fixed in a branch, landed, and
   a new tag is made (`-rc.N+1`, or a new patch version).

## Consequences

- One CI run per check tag instead of one per landing; GitHub's `main` lags until the next tag.
- A problem only a clean container or the release profile shows (ADR-0016) is found at the next
  tag or the weekly run. Land with `--release` when a change is near `cfg!(debug_assertions)` or
  build profiles; both scripts land their bump with `--release`.
- Every check tag is a version: `Cargo.toml` says `0.4.0-rc.2` between releases, and the status
  bar shows it.

## Alternatives

- **Keep CI on `main`**: a check after each landing, at the cost of the queue.
- **One tag, two steps** (tag on Forgejo, a second script pushes it to GitHub when green): one
  version per release but no check of `main` without a release.
- **Check tags without a version** (`check-<date>`): the owner wants a version on every tag.

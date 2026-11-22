# ADR-0025: GitHub checks macOS and Windows on `main`

- **Status**: Accepted (owner decision, 2026-11-21)
- **Date**: 2026-11-21
- **Specs**: FORGE-003, FORGE-014, FORGE-020, BUILD-005, BUILD-012, BUILD-016, BUILD-017, BUILD-039
- **Issue**: #452, #453
- **Amends**: [ADR-0017](0017-github-issues-forgejo-ci.md) §4 ("GitHub builds only releases")

## Context

- **Releases failed on GitHub.** 4 of the last 5 releases were red there, all on macOS: v0.6.7
  (`media::output::tests::the_clock_card_keeps_real_time_when_its_sleeps_run_long`) and v0.6.8 (the
  `build_025_*` install-script tests) were real macOS bugs that `scripts/land.sh` cannot catch
  (`cross-check.sh` runs clippy only). Each red release costs about an hour and a version number
  (#453).
- **The Rust cache never hits.** `build.yml` runs only for tags, and a GitHub cache is scoped to its
  ref: only caches saved on the default branch (`main`) can be read by other refs. Every tag saved
  into its own scope, so the next tag started cold ("No cache found"); Windows took 31 min (build 11,
  test 14), macOS and Linux about 18 (#452).
- `main` reaches GitHub only from Forgejo's `mirror` job, after a green Forgejo run for a check tag
  `vX.Y.Z-rc.N` or a release (ADR-0018, BUILD-016). So a push of `main` to GitHub happens per
  check tag, not per landing.

## Decision

1. **On each push of `main` to GitHub, a workflow tests macOS and Windows.**
   `.github/workflows/main-check.yml` (`push` to `main`, and `workflow_dispatch`; `contents: read`
   only; a newer push cancels an older run) has the jobs `macos` and `windows` with the commands and
   environment of `build.yml` (`cargo build --release --locked`, `cargo test --release --locked`,
   `RUSTFLAGS=-D warnings`, same toolchain and system setup), and a job `linux` that only builds
   (`cargo build --release --locked`, `cargo test --release --locked --no-run`) to warm Linux's
   cache: Forgejo ran the Linux tests on the commit already (BUILD-012).
2. **The cache is saved on `main` and restored by the release build.** All `Swatinem/rust-cache` steps
   (same pinned commit) use `shared-key: release-<OS>` in both workflows; the job name would
   otherwise be part of the key and `build.yml` could not restore what `main-check.yml` saved. The
   saving is `save-if: github.ref == 'refs/heads/main'`, so tag runs restore and never save.
3. **A release is not tested twice.** For a release, the `mirror` job pushes the tag to GitHub
   *before* `main`; the first job of `main-check.yml` (`gate`) then finds a release tag
   (`vX.Y.Z`, no `-`) at the pushed commit and skips the tests, as `build.yml` runs them. A check tag
   stays on Forgejo, so its push of `main` is tested. A run by hand is never skipped. A skipped run
   ends green without having tested anything; the release build has tested that commit, and the next
   check tag's run replaces it as the latest.
   *Not chosen*: letting both run (about 30 extra runner minutes per release on macOS and Windows).
4. **`make-release.sh` looks at that run** (FORGE-020): it stops when the latest main-check run on
   GitHub's `main` is red or still running, when `gh` cannot answer, and says so; `--ignore-github-check`
   skips it, `--dry-run` reports it. The release commit itself is not on GitHub yet, so the latest run
   is the one of the last check tag; a macOS or Windows failure is found there, before a release is cut.

## What is not changed

- Branches are never pushed to GitHub, and no pull requests are opened by us there (ADR-0017 §6).
- Issues stay on GitHub, git and the day-to-day CI on Forgejo; a landing starts nothing on GitHub.
- Releases are built, attested and published by `build.yml` as before (BUILD-012, BUILD-014).
- No clippy, fmt or Linux tests on GitHub.

## Consequences

- **Cost**: public-repository runner minutes (free, but the owner wanted the fewest GitHub builds):
  about 15 to 30 minutes per check tag across macOS, Windows and Linux, in parallel. Never more than
  one run at a time per newer push (concurrency cancel).
- A macOS or Windows break shows up at the check tag, in the minutes after Forgejo's run, not at the
  release.
- Release builds should restore a cache from the last check tag's run; the Windows job should be
  clearly shorter (the aim of #452). A changed `Cargo.lock`, toolchain or `RUSTFLAGS` misses the exact
  key; rust-cache then falls back to the newest cache of the same `shared-key`.
- The check tag before a release must have gone green before `make-release.sh`, or the owner says
  `--ignore-github-check`.
- The cache of a red macOS or Windows run is saved too (`cache-on-failure`), so a fix starts warm.

## Alternatives

- **Test on every landing**: the burden ADR-0017 and ADR-0018 removed.
- **Cache from tag runs only**: never hits, see Context.
- **Fail make-release.sh only when it can see a red run for the commit it releases**: that commit is
  not on GitHub until the release's own Forgejo run is green, so it cannot be known earlier.

## Note, 2026-11-22 (#546)

The `gate` job skips main-check's jobs for a commit with a release tag, so the run on a release commit
is green without a test, and `make-release.sh` (FORGE-020) cannot see a failure that is not there.
Commits since the last check tag that reached GitHub may never have run on macOS or Windows before the
release; the release build is their first test. **Decided by the owner (option c)**: the release
build's own macOS and Windows tests (`build.yml`, BUILD-017, BUILD-024) are the gate, and a failure
there stops publishing. Rejected: (a) require a green main-check of the exact tree before the release
(that tree reaches GitHub only with the release); (b) let main-check run on release commits too (the
same tests would run twice, which is what the gate avoids).

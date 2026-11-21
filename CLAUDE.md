# Working on Shale

Shale is a native Rust notes app. These rules are binding; the full versions are spec rows in
[`docs/spec/engineering.md`](docs/spec/engineering.md) (FORGE, DEV, BUILD) and the decisions in
[`docs/decisions/`](docs/decisions/README.md), above all
[ADR-0001](docs/decisions/0001-spec-driven-development.md) (spec-driven) and
[ADR-0013](docs/decisions/0013-forgejo-and-issue-tracking.md) with
[ADR-0017](docs/decisions/0017-github-issues-forgejo-ci.md) (Forgejo for git and CI, GitHub for issues).

## Forgejo, GitHub and issues (FORGE)

- **Issues are on GitHub** (`rabalyn/shale`, remote `github`), worked with `gh`
  (`gh issue list -R rabalyn/shale`, `gh issue view 5 -R rabalyn/shale`, `gh issue create`,
  `gh issue comment`, `gh issue close`). Issues are written `#N`; `raba/shale#N` in older text
  is the same issue (the Forgejo issues were imported with their numbers, FORGE-012,
  [ADR-0017](docs/decisions/0017-github-issues-forgejo-ci.md)). Never open pull requests on GitHub
  for your own work, and never push branches there.
- **Git and CI are on Forgejo**: <https://git.home.rabalyn.de/raba/shale> (remote `origin`, the
  owner's homelab, not public), which has no issues or pull requests any more. Its CI runs **only
  for `v*` tags** (and weekly), not for a push to `main`; for a green tag its `mirror` job pushes
  `main` to GitHub (where `main-check.yml` tests macOS and Windows and warms the Rust cache,
  BUILD-038, [ADR-0025](docs/decisions/0025-github-checks-main.md)), and a release tag too, which GitHub builds and publishes (FORGE-002, FORGE-003,
  BUILD-016, [ADR-0018](docs/decisions/0018-tag-only-ci.md)). `fj --host git.home.rabalyn.de` is only for CI
  runs (`fj … actions tasks`), not for issues.
- **Before you start**: read the open issues of your area (`gh issue list -R rabalyn/shale`,
  `--label security` for findings of `cargo audit`, BUILD-021). If the work has no issue, create
  one first.
- **Every open to-do is a GitHub issue**: owner notes, SHOULD BE spec rows with planned work,
  bugs and follow-ups you find in passing. Do not leave to-dos only in chat, in a file or in
  `docs/spec/open-points.md` (that file holds undecided questions only).
- **Commits name their issue**: `Refs #N`, or `Closes #N` in the change that finishes it (it
  closes the issue when `main` reaches GitHub); `no-issue:` only for a typo or formatting fix.
- **Done work is reflected**: close the finished issue (`Closes #N`, or `gh issue close N -c …`
  naming the commit); comment on an issue when you make partial progress or the plan changes. A
  SHOULD BE row names its issue; flipping it to IS NOW closes the issue.
- **Your final report** lists the issues you opened, commented on or closed.
- **Landing changes** (FORGE-010, FORGE-011, [ADR-0016](docs/decisions/0016-local-landing.md)):
  `main` changes only through `scripts/land.sh <branch>…`, which merges each branch into the
  current `origin/main`, runs fmt, clippy (Linux, and macOS and Windows via `scripts/cross-check.sh`), `cargo test` (and the podman suites when CalDAV or mail
  code changed) and pushes `main` when all pass; one landing at a time. It then fast-forwards the
  local `main` (when its checkout is clean), so new sessions start from the landed state. Push
  your branch (or keep it local), then land it; there is no CI run to wait for. `land.sh` is the
  only check before `main`, so never skip it. A conflict:
  merge `origin/main` into your branch, resolve it there (keep both sides; renumber a clashing
  spec ID on your side) and land again. A change near `cfg!(debug_assertions)` or build profiles:
  `land.sh --release`. A red `main` is fixed first. Pull requests are optional.
- **Tags and releases are made only by script, and only when the owner asks** (ADR-0018):
  - `scripts/make-tag.sh [patch|minor|major]` makes a **check tag** `vX.Y.Z-rc.N` for "done for
    now" (FORGE-013): it bumps `Cargo.toml`/`Cargo.lock`, merges `docs/changelog-pending/` into
    `[Unreleased]`, lands the bump, tags the landed commit and pushes the tag to Forgejo, whose CI
    runs once; when green, `main` goes to GitHub (only `main-check.yml` runs there: macOS and Windows tests).
  - `scripts/make-release.sh [patch|minor|major]` makes a **release** `vX.Y.Z` (FORGE-014): the
    version bump, `[Unreleased]` and `docs/changelog-pending/` merged into the version's
    `CHANGELOG.md` section, the release check (BUILD-013), landing, tag; when Forgejo is green,
    `main` and the tag go to GitHub, which publishes the release.
  - Both stop with "the changelog needs an update first" when there is nothing new in the
    changelog (a check tag: no change since the previous tag; a release: no entries), so every
    change carries its `docs/changelog-pending/` entry.
  - `make-release.sh` also stops when GitHub's latest `main-check` run is red or still running
    (FORGE-017); `--ignore-github-check` skips that.
  - Both name the issue `Release X.Y.Z` (made when missing) and accept `--dry-run`. Never tag by
    hand, never move or delete a pushed tag: a red run is fixed in a branch, landed, then the next
    tag is made.

## Specs first (ADR-0001)

- Change the spec in `docs/spec/` before the code; flip `SHOULD BE` → `IS NOW` in the change
  that makes it true, with the test name in the Test column.
- Tests that check a rule carry its ID in the name or a `// spec: ID` comment.

## Code and tests (DEV, BUILD)

- **No warnings.** `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`,
  `cargo build` and `cargo test` must be clean. If a warning cannot be fixed cleanly, stop and
  ask the owner (and record it in an issue); do not silence it with `#[allow]` without their OK,
  and do not loop on it.
- **No dead code**, no unused dependencies.
- **Every code path gets a sensible test.** External behaviour (IMAP, CalDAV, …) is tested
  against real servers in podman test stacks with seeded data (`tests/mail-env/`).
- **If a good test is hard to write**, first think about developer tooling (a test server,
  fixture generator, debug setting or helper tool) and propose it to the owner as a side track
  (an issue) before writing a shallow test.
- Keep the code modular: feature folders, small sub-states instead of more fields on `ShaleApp`.
- **Docs and screenshots**: every change checks README, `docs/FEATURES.md`, the specs,
  `CHANGELOG.md` (or `docs/changelog-pending/`) and the screenshots in `docs/screenshots/`, and
  updates what it made stale.
- **User guide**: the user guide is a Shale vault (DEV-018, raba/shale#182). Every change that
  adds, changes or removes a feature or a setting updates the guide's notes, its settings
  reference, and the screenshots and videos that show it, in the same change.
- **Releases**: every tag bumps the version (semver, `Cargo.toml` and `Cargo.lock`); a release has
  notes in `CHANGELOG.md` for everything since the previous release. Use `scripts/make-tag.sh` and
  `scripts/make-release.sh`, which do both (see Forgejo above).

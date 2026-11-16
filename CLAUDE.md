# Working on Shale

Shale is a native Rust notes app. These rules are binding; the full versions are spec rows in
[`docs/spec/engineering.md`](docs/spec/engineering.md) (FORGE, DEV, BUILD) and the decisions in
[`docs/decisions/`](docs/decisions/README.md), above all
[ADR-0001](docs/decisions/0001-spec-driven-development.md) (spec-driven) and
[ADR-0013](docs/decisions/0013-forgejo-and-issue-tracking.md) (Forgejo and issues).

## Forgejo and issues (FORGE)

- Development happens on Forgejo: <https://git.home.rabalyn.de/raba/shale> (remote `origin`). Use the Forgejo CLI
  with the host given explicitly: `fj --host git.home.rabalyn.de …` (signed in with a token;
  issues are addressed as `raba/shale#N`, e.g. `fj --host git.home.rabalyn.de issue view raba/shale#5`).
  GitHub (`rabalyn/shale`, remote `github`) builds and publishes releases (`v*` tags, cut
  now and then via `make-release.sh`) and tests macOS and Windows on `main`, which Forgejo's
  `mirror` job pushes there after its CI passed (FORGE-003, BUILD-016). Never push branches or
  day-to-day work there yourself (a test branch for a run by hand only when the owner asks, and
  deleted afterwards), and never open issues or PRs there.
- **Before you start**: read the open issues of your area
  (`fj --host git.home.rabalyn.de issue search -r raba/shale`). If the work has no issue, create
  one first.
- **Every open to-do is a Forgejo issue**: owner notes, SHOULD BE spec rows with planned work,
  bugs and follow-ups you find in passing. Do not leave to-dos only in chat, in a file or in
  `docs/spec/open-points.md` (that file holds undecided questions only).
- **Commits name their issue**: `Refs #N`, or `Closes #N` in the change that finishes it;
  `no-issue:` only for a typo or formatting fix.
- **Done work is reflected**: close the finished issue (`Closes #N`, or `fj … issue close` with a
  comment naming the commit); comment on an issue when you make partial progress or the plan
  changes. A SHOULD BE row names its issue; flipping it to IS NOW closes the issue.
- **Your final report** lists the issues you opened, commented on or closed.
- **Landing changes** (FORGE-010, FORGE-011, [ADR-0016](docs/decisions/0016-local-landing.md)):
  `main` changes only through `scripts/land.sh <branch>…`, which merges each branch into the
  current `origin/main`, runs fmt, clippy, `cargo test` (and the podman suites when CalDAV or mail
  code changed) and pushes `main` when all pass; one landing at a time. Push your branch (or keep
  it local), then land it; do not wait for CI, which runs only on `main` afterwards. A conflict:
  merge `origin/main` into your branch, resolve it there (keep both sides; renumber a clashing
  spec ID on your side) and land again. A change near `cfg!(debug_assertions)` or build profiles:
  `land.sh --release`. A red `main` is fixed first. Pull requests are optional.

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
- **Releases**: a tag bumps the version correctly (semver, `Cargo.toml` and `Cargo.lock`) and has
  release notes in `CHANGELOG.md` for everything since the previous tag; use `make-release.sh`.

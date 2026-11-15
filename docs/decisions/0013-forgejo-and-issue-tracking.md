# ADR-0013: Forgejo is the home of development; its issues track all work; GitHub only distributes releases

- **Status**: Accepted (owner decision, 2026-11-15)
- **Date**: 2026-11-15
- **Specs**: FORGE-001…FORGE-009, DEV-001…DEV-008, BUILD-013
- **Issue**: Forgejo raba/shale#1

## Context

- Shale has two remotes: `origin` (GitHub, `rabalyn/shale`) and `forgejo`
  (`ssh://git@git.home.rabalyn.de:2222/raba/shale.git`). The Forgejo repository exists and the
  Forgejo CLI `fj` is signed in with a token (`fj --host git.home.rabalyn.de …`).
- Today all CI runs on GitHub Actions (`.github/workflows/build.yml`): checks, the `latest`
  pre-release and tagged releases (BUILD-002, BUILD-003).
- Open work was spread over `docs/spec/open-points.md`, the backlog in IMPLEMENTATION.md, the
  owner's `TO-TELL.md` notes and chat. Nothing said which list was authoritative, and finished
  work was not reliably reflected anywhere but the changelog.

## Decision

1. **Forgejo is the primary forge.** Branches, pull requests, reviews and issues live on
   `git.home.rabalyn.de/raba/shale`. Agents and people use `fj --host git.home.rabalyn.de`
   (or the Forgejo API with the same token) for issues and PRs.
2. **Forgejo builds.** Pushes and PRs run the checks (fmt, clippy `-D warnings`, build, tests,
   audit) on the Forgejo runner. A `v*` tag pushed to Forgejo starts the runner that checks the
   version and notes and builds the release artifacts.
3. **GitHub only distributes releases**, through a separate release workflow that publishes a
   tagged, Forgejo-built release (tarball, checksum, notes) to GitHub Releases. GitHub runs no
   build or test CI of its own. How the source reaches GitHub (push mirror or tags only) is
   settled in the migration issue (raba/shale#2).
4. **Forgejo issues are the one to-do list.** Every open to-do (a SHOULD BE spec row with
   planned work, a bug, an owner note, a follow-up found while working) is an open issue.
   `open-points.md` keeps undecided *questions*; `TO-TELL.md` and similar notes are input that
   is turned into issues, not a second list.
5. **Work is reflected in the issues.** Work starts from an issue (created first if none
   exists), commits name it (`Refs #N`), the change that finishes it closes it (`Closes #N`
   or `fj issue close` with a comment naming the commit), and partial progress or a changed
   plan is a comment. A SHOULD BE row names its issue; flipping it to IS NOW closes the issue.
6. **Enforced by specs and, later, by CI**: the FORGE rules in
   [`engineering.md`](../spec/engineering.md) are binding for people and agents; a CI check
   that every commit references an issue follows with the Forgejo CI (FORGE-008, raba/shale#4).
7. The owner's development rules from `TO-TELL.md` (no warnings, escalate the ones that cannot
   be fixed; no dead code; a sensible test for every code path; real servers in podman for
   external behaviour; tooling as a proposed side track before a shallow test; docs and
   screenshots checked on every change; correct version bumps and release notes) become the
   DEV rules and BUILD-013, each SHOULD BE row linked to its issue.

## Consequences

- One place answers "what is open?" and "what was done?": the Forgejo issue list and its
  closed issues.
- Every change carries a bit of bookkeeping (create, reference, comment, close). That is the
  price for not losing to-dos.
- Issue numbers in commit messages refer to Forgejo. If the source is mirrored to GitHub, a bare
  `#N` there points at GitHub's numbering; release notes and docs therefore write Forgejo issues
  as `raba/shale#N` or as a full link.
- Since the CI migration (BUILD-002, BUILD-012), day-to-day CI runs on Forgejo; GitHub still
  rebuilds release tags itself, so FORGE-003 stays SHOULD BE until raba/shale#2 publishes the
  Forgejo build instead.

- Forgejo availability now matters for development; GitHub remains the public download place.

## Alternatives

- **GitHub as the primary forge** (status quo): public, but the owner develops against their
  own Forgejo and wants CI on their own runner.
- **Keep the work list in `open-points.md`**: versioned with the code, but cannot be assigned,
  linked to commits or closed by them, and drifted before.

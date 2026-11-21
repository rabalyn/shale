# ADR-0017: GitHub is the public home and holds the issues; Forgejo keeps git and CI; GitHub builds only releases

- **Status**: Accepted (owner decision, 2026-11-17)
- **Date**: 2026-11-17
- **Specs**: FORGE-001, FORGE-003…FORGE-009, FORGE-012, BUILD-005, BUILD-009, BUILD-012, BUILD-016, BUILD-017, BUILD-020 (retired), BUILD-021, BUILD-022, DEV-009
- **Issue**: #256 (security scanning: #257)
- **Amends**: [ADR-0013](0013-forgejo-and-issue-tracking.md) §1, §3, §4, §5
- **Amended by**: [ADR-0018](0018-tag-only-ci.md); [ADR-0025](0025-github-checks-main.md) §4: GitHub also tests macOS and Windows on each push of `main` (per green check tag) and saves the Rust cache there

## Context

- The homelab Forgejo (`git.home.rabalyn.de/raba/shale`) can be reached only by the owner. The
  public repository `rabalyn/shale` on GitHub is the only place anyone else can see, so a person
  with a problem opens the issue there.
- Under ADR-0013 the issues lived on Forgejo. Two trackers with their own numbering would drift:
  `#57` in a commit would mean one thing on Forgejo and another on GitHub.
- Forgejo was chosen so the free public runners of GitHub are not used for day-to-day builds. The
  owner wants the fewest GitHub builds possible: since raba/shale#26 GitHub tested macOS and
  Windows for every push to `main`; that is more than releases need.

## Decision

1. **Issues live on GitHub** (`rabalyn/shale`), and they are the one to-do list (the rules of
   ADR-0013 §4 and §5 stay, with GitHub in place of Forgejo). People and agents use `gh` (or the
   GitHub API) for issues. Issues are written `#N`.
2. **The numbers are the same as before.** On 2026-11-17 the GitHub repository was recreated empty
   (it had no stars or forks; its two old pull requests are in the git history), the git history
   and the releases were put back, and every Forgejo issue and pull request was imported in order,
   so that GitHub's `#N` is Forgejo's `#N` (pull requests became closed issues labelled
   `forgejo-pr`; each import names its original author and dates). Then the issues and pull
   requests of the Forgejo repository were switched off, so no new number can come up there.
   `raba/shale#N` in older text and commits means the same issue as `#N`.
3. **Forgejo keeps git and CI.** `origin` stays Forgejo; changes land locally (ADR-0016) and the
   homelab runner checks `main` afterwards. A green `main` is pushed to GitHub's `main` by the
   `mirror` job, which runs no workflow there. A commit with `Closes #N` closes the issue on GitHub
   when it arrives there. The direction is not reversed (GitHub first, Forgejo mirroring it),
   because then GitHub would see every change before the homelab checked it, and a pull mirror
   does not start Forgejo's CI.
4. **GitHub builds only releases** (*amended by ADR-0025: and checks macOS and Windows on `main`, `.github/workflows/main-check.yml`*): `.github/workflows/build.yml` runs for `v*` tags and by hand
   (to try `main` on macOS and Windows before a release), never for a push to `main`. Nothing is
   reported back from GitHub runs any more (BUILD-020 retired): a release is cut by hand and
   watched, and GitHub's notification of a failed run reaches the owner.
   *Note (#454): the workflow does not repeat checks that ran on the same commit: no clippy or
   fmt (`land.sh`), and no Linux tests (Forgejo, before the tag is mirrored); see BUILD-012.*
5. **Security findings become issues** (#257): a daily Forgejo workflow (`security.yml`, and for
   every push to `main`) runs `cargo audit` and opens one GitHub issue per finding, closes it when
   the finding is gone, and leaves one closed as *not planned* alone (BUILD-021). GitHub's own
   free features, which use no runner, are on: Dependabot alerts, secret scanning with push
   protection and private vulnerability reporting, with `SECURITY.md` saying how to report
   (BUILD-022).
6. Pull requests from outside come on GitHub; the owner's own branches are landed locally and need
   no pull request.

## Consequences

- One tracker and one numbering, public; the numbers in nine days of commits stay right.
- Forgejo's issue history exists on GitHub only as imports: author and dates are in a line at the
  top of each, not in GitHub's own fields.
- The token of the `mirror` job (`MIRROR_DEPLOY_KEY`) also needs Issues read and write, for the
  security report.
- macOS and Windows are tested only for releases and by hand; a break there shows up at the next
  release, or when the owner runs the workflow by hand.

## Alternatives

- **Mirror Forgejo's issues to GitHub, one way**: outsiders' GitHub issues would never reach
  Forgejo, and the numbers would differ.
- **Keep both trackers and sync both ways**: two numberings that cannot be kept equal.
- **Move git to GitHub as well**: see Decision 3.
